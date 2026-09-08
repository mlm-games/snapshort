use crate::services::project_snapshot::{
    clear_autosave, read_autosave_meta, read_autosave_snapshot, read_snapshot_report,
    write_snapshot,
};
use crate::ProjectSnapshot;
use crate::{
    AppError, AppEvent, AppResult, Asset, AssetId, EventBus, ProjectCommand, TimelineMarkerData,
};
use miniter_domain::{Project, Timeline, Timestamp};
use miniter_usecases::reducer::{dispatch_labeled, redo, undo};
use miniter_usecases::EditorState;
use game_utils::storage::{FsStorage, Storage};
use snapshort_infra_store::ProjectStore;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, instrument};

pub struct ProjectService {
    project_store: ProjectStore,
    event_bus: EventBus,
    editor: Arc<RwLock<Option<EditorState>>>,
    assets: Arc<RwLock<HashMap<AssetId, Asset>>>,
    project_path: Arc<RwLock<Option<PathBuf>>>,
    data_dir: PathBuf,
}

/// Crash-recovery offer surfaced at boot: a valid autosave newer than the
/// last explicit save.
#[derive(Debug, Clone)]
pub struct AutosaveFound {
    pub project_name: String,
    pub saved_at_ms: i64,
    pub project_path: Option<PathBuf>,
}

impl ProjectService {
    pub fn new(store: ProjectStore, event_bus: EventBus, data_dir: PathBuf) -> Self {
        Self {
            project_store: store,
            event_bus,
            editor: Arc::new(RwLock::new(None)),
            assets: Arc::new(RwLock::new(HashMap::new())),
            project_path: Arc::new(RwLock::new(None)),
            data_dir,
        }
    }

    pub async fn editor(&self) -> Arc<RwLock<Option<EditorState>>> {
        self.editor.clone()
    }

    pub async fn assets(&self) -> Arc<RwLock<HashMap<AssetId, Asset>>> {
        self.assets.clone()
    }

    pub async fn current_project(&self) -> Option<Project> {
        self.editor.read().await.as_ref().map(|e| e.project.clone())
    }

    pub async fn current_timeline(&self) -> Option<Timeline> {
        self.editor
            .read()
            .await
            .as_ref()
            .map(|e| e.project.timeline.clone())
    }

    pub async fn current_playhead(&self) -> Timestamp {
        self.editor
            .read()
            .await
            .as_ref()
            .map(|e| e.playhead)
            .unwrap_or(Timestamp::ZERO)
    }

    pub async fn current_path(&self) -> Option<PathBuf> {
        self.project_path.read().await.clone()
    }

    pub async fn list_assets(&self) -> Vec<Asset> {
        self.assets.read().await.values().cloned().collect()
    }

    pub async fn get_asset(&self, id: AssetId) -> Option<Asset> {
        self.assets.read().await.get(&id).cloned()
    }

    pub async fn add_asset(&self, asset: Asset) {
        self.assets.write().await.insert(asset.id, asset);
    }

    pub async fn remove_asset(&self, id: AssetId) -> Option<Asset> {
        self.assets.write().await.remove(&id)
    }

    pub async fn load_assets(&self, assets: Vec<Asset>) {
        let mut store = self.assets.write().await;
        store.clear();
        for asset in assets {
            store.insert(asset.id, asset);
        }
    }

    #[instrument(skip(self))]
    pub async fn execute(&self, command: ProjectCommand) -> AppResult<()> {
        match command {
            ProjectCommand::Create { name } => {
                self.create_project(name).await?;
            }
            ProjectCommand::Open { path } => {
                self.open_project(path).await?;
            }
            ProjectCommand::Save { markers } => {
                self.save_project(markers).await?;
            }
            ProjectCommand::SaveAs { path, markers } => {
                self.save_project_as(path, markers).await?;
            }
            ProjectCommand::Close => {
                self.close_project().await?;
            }
            ProjectCommand::RestoreAutosave => {
                self.restore_autosave().await?;
            }
            ProjectCommand::DiscardAutosave => {
                clear_autosave(&self.data_dir);
            }
        }
        Ok(())
    }

    pub async fn dispatch_timeline_command(
        &self,
        cmd: miniter_usecases::EditCommand,
        label: String,
    ) -> AppResult<()> {
        let (project, can_undo, can_redo) = {
            let mut guard = self.editor.write().await;
            let mut editor = guard.take()
                .ok_or_else(|| AppError::Other("No project open".into()))?;

            dispatch_labeled(&mut editor, label, cmd)?;

            let result = (
                editor.project.clone(),
                editor.history.can_undo(),
                editor.history.can_redo(),
            );
            *guard = Some(editor);
            result
        };

        // The reducer guarantees these; re-check at the point of use so a
        // regression surfaces here in dev, not as a corrupt render downstream.
        debug_assert!(
            crate::validate_timeline(&project.timeline).is_empty(),
            "timeline invariants violated: {:?}",
            crate::validate_timeline(&project.timeline)
        );

        self.event_bus
            .emit(AppEvent::TimelineUpdated { timeline: project.timeline });
        let (undo_label, redo_label) = self.undo_labels().await;
        self.event_bus.emit(AppEvent::UndoStackChanged {
            can_undo,
            can_redo,
            undo_label,
            redo_label,
        });

        Ok(())
    }

    pub async fn undo_timeline(&self) -> AppResult<()> {
        let (project, can_undo, can_redo) = {
            let mut guard = self.editor.write().await;
            let mut editor = guard.take()
                .ok_or_else(|| AppError::Other("No project open".into()))?;

            undo(&mut editor)?;

            let result = (
                editor.project.clone(),
                editor.history.can_undo(),
                editor.history.can_redo(),
            );
            *guard = Some(editor);
            result
        };

        // The reducer guarantees these; re-check at the point of use so a
        // regression surfaces here in dev, not as a corrupt render downstream.
        debug_assert!(
            crate::validate_timeline(&project.timeline).is_empty(),
            "timeline invariants violated: {:?}",
            crate::validate_timeline(&project.timeline)
        );

        self.event_bus
            .emit(AppEvent::TimelineUpdated { timeline: project.timeline });
        let (undo_label, redo_label) = self.undo_labels().await;
        self.event_bus.emit(AppEvent::UndoStackChanged {
            can_undo,
            can_redo,
            undo_label,
            redo_label,
        });

        Ok(())
    }

    pub async fn redo_timeline(&self) -> AppResult<()> {
        let (project, can_undo, can_redo) = {
            let mut guard = self.editor.write().await;
            let mut editor = guard.take()
                .ok_or_else(|| AppError::Other("No project open".into()))?;

            redo(&mut editor)?;

            let result = (
                editor.project.clone(),
                editor.history.can_undo(),
                editor.history.can_redo(),
            );
            *guard = Some(editor);
            result
        };

        // The reducer guarantees these; re-check at the point of use so a
        // regression surfaces here in dev, not as a corrupt render downstream.
        debug_assert!(
            crate::validate_timeline(&project.timeline).is_empty(),
            "timeline invariants violated: {:?}",
            crate::validate_timeline(&project.timeline)
        );

        self.event_bus
            .emit(AppEvent::TimelineUpdated { timeline: project.timeline });
        let (undo_label, redo_label) = self.undo_labels().await;
        self.event_bus.emit(AppEvent::UndoStackChanged {
            can_undo,
            can_redo,
            undo_label,
            redo_label,
        });

        Ok(())
    }

    pub async fn set_playhead(&self, timestamp: Timestamp) {
        let mut editor_lock = self.editor.write().await;
        if let Some(editor) = editor_lock.as_mut() {
            editor.playhead = timestamp;
        }
        self.event_bus.emit(AppEvent::PlayheadMoved {
            timestamp,
            dropped_total: 0,
        });
    }

    #[instrument(skip(self))]
    async fn create_project(&self, name: String) -> AppResult<Project> {
        let project = Project::new(&name);
        let editor = EditorState::new(project.clone());

        self.project_store.save(&project)?;
        *self.editor.write().await = Some(editor);
        *self.project_path.write().await = None;
        self.assets.write().await.clear();

        let timeline = project.timeline.clone();
        self.event_bus
            .emit(AppEvent::ProjectCreated { project: project.clone() });
        self.event_bus
            .emit(AppEvent::TimelineUpdated { timeline });

        info!("Created project: {}", name);
        Ok(project)
    }

    #[instrument(skip(self))]
    async fn open_project(&self, path: PathBuf) -> AppResult<Project> {
        let path = normalize_project_path(path);
        let (snapshot, stripped) = read_snapshot_report(&path)?;
        let project = self.install_snapshot(snapshot, Some(path)).await;
        if !stripped.is_empty() {
            self.event_bus.emit(AppEvent::EffectsStripped {
                filters: stripped.filters,
                clips: stripped.clips,
                transitions: stripped.transitions,
            });
        }
        project
    }

    /// Install an already-loaded snapshot as the open project (file opens and
    /// autosave restores share this path).
    async fn install_snapshot(
        &self,
        snapshot: ProjectSnapshot,
        path: Option<PathBuf>,
    ) -> AppResult<Project> {
        let mut assets = HashMap::new();
        for asset in snapshot.assets {
            assets.insert(asset.id, asset);
        }

        let editor = EditorState::new(snapshot.project.clone());
        *self.editor.write().await = Some(editor);
        *self.assets.write().await = assets;
        *self.project_path.write().await = path.clone();

        let timeline = snapshot.project.timeline.clone();
        self.event_bus.emit(AppEvent::ProjectOpened {
            project: snapshot.project.clone(),
            timeline_markers: snapshot.timeline_markers.clone(),
        });
        self.event_bus
            .emit(AppEvent::AssetsLoaded { assets: self.list_assets().await });
        self.event_bus
            .emit(AppEvent::TimelineUpdated { timeline });

        info!("Opened project: {}", snapshot.project.meta.name);
        Ok(snapshot.project)
    }

    /// Current in-memory state as a crash-recovery snapshot. Timeline markers
    /// live in UI state and are intentionally excluded (matches web autosave).
    pub async fn autosave_snapshot(&self) -> Option<(Option<PathBuf>, ProjectSnapshot)> {
        let editor = self.editor.read().await.as_ref().cloned()?;
        let assets: Vec<Asset> = self.assets.read().await.values().cloned().collect();
        let path = self.project_path.read().await.clone();
        Some((
            path,
            ProjectSnapshot::new(editor.project, assets, Vec::new()),
        ))
    }

    /// A restorable autosave, if one exists and is newer than the last
    /// explicit save. Stale copies (saved-after) are deleted silently.
    pub async fn check_autosave(&self) -> Option<AutosaveFound> {
        let data_dir = &self.data_dir;
        let meta = read_autosave_meta(data_dir)?;
        let snapshot = read_autosave_snapshot(data_dir)?;
        if let Some(ref project_path) = meta.project_path {
            let mtime_ms = FsStorage
                .mtime_secs(project_path)
                .map(|s| s as i64 * 1000);
            match mtime_ms {
                // Project file saved after the crash copy: stale, drop it.
                Some(mtime) if mtime >= meta.saved_at_ms => {
                    clear_autosave(data_dir);
                    return None;
                }
                // Project file gone (deleted/moved): the snapshot may be all
                // that's left — still offer it. Missing mtime falls through.
                _ => {}
            }
        }
        Some(AutosaveFound {
            project_name: snapshot.project.meta.name.clone(),
            saved_at_ms: meta.saved_at_ms,
            project_path: meta.project_path,
        })
    }

    #[instrument(skip(self))]
    async fn restore_autosave(&self) -> AppResult<Project> {
        let meta = read_autosave_meta(&self.data_dir)
            .ok_or_else(|| AppError::InvalidInput("No autosave found".into()))?;
        let snapshot = read_autosave_snapshot(&self.data_dir)
            .ok_or_else(|| AppError::InvalidInput("Autosave is corrupt".into()))?;
        let project = self
            .install_snapshot(snapshot, meta.project_path)
            .await?;
        clear_autosave(&self.data_dir);
        info!("Restored project from autosave: {}", project.meta.name);
        Ok(project)
    }

    #[instrument(skip(self))]
    async fn save_project(&self, markers: Vec<TimelineMarkerData>) -> AppResult<()> {
        let project_path = self
            .project_path
            .read()
            .await
            .clone()
            .ok_or_else(|| AppError::InvalidInput("No file path set. Use Save As first.".into()))?;

        let editor = self
            .editor
            .read()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| AppError::ProjectNotFound(uuid::Uuid::nil()))?;

        let assets: Vec<Asset> = self.assets.read().await.values().cloned().collect();
        let snapshot = ProjectSnapshot::new(editor.project.clone(), assets, markers);
        write_snapshot(&project_path, &snapshot)?;
        // The shadow copy is now stale by definition.
        clear_autosave(&self.data_dir);

        self.project_store.save(&editor.project)?;

        self.event_bus
            .emit(AppEvent::ProjectSaved { path: project_path });

        let project = editor.project;
        info!("Saved project: {}", project.meta.name);
        Ok(())
    }

    #[instrument(skip(self))]
    async fn save_project_as(&self, path: PathBuf, markers: Vec<TimelineMarkerData>) -> AppResult<()> {
        let path = normalize_project_path(path);
        let editor = self
            .editor
            .read()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| AppError::ProjectNotFound(uuid::Uuid::nil()))?;

        let assets: Vec<Asset> = self.assets.read().await.values().cloned().collect();
        let snapshot = ProjectSnapshot::new(editor.project.clone(), assets, markers);
        write_snapshot(&path, &snapshot)?;
        clear_autosave(&self.data_dir);

        *self.project_path.write().await = Some(path.clone());
        self.project_store.save(&editor.project)?;

        self.event_bus.emit(AppEvent::ProjectSaved { path });

        info!("Saved project as: {}", editor.project.meta.name);
        Ok(())
    }

    #[instrument(skip(self))]
    async fn close_project(&self) -> AppResult<()> {
        *self.editor.write().await = None;
        self.assets.write().await.clear();
        *self.project_path.write().await = None;

        self.event_bus.emit(AppEvent::ProjectClosed);
        info!("Closed project");
        Ok(())
    }

    /// Next undo/redo labels, if the top steps carry gesture labels.
    async fn undo_labels(&self) -> (Option<String>, Option<String>) {
        let guard = self.editor.read().await;
        let Some(editor) = guard.as_ref() else {
            return (None, None);
        };
        fn non_empty(label: Option<&str>) -> Option<String> {
            label.filter(|s| !s.is_empty()).map(str::to_string)
        }
        (
            non_empty(editor.history.undo_label()),
            non_empty(editor.history.redo_label()),
        )
    }
    pub async fn list_projects(&self) -> AppResult<Vec<Project>> {
        Ok(self.project_store.list()?)
    }
}

fn normalize_project_path(path: PathBuf) -> PathBuf {
    if path.extension().is_some() {
        path
    } else {
        path.with_extension("snap")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use crate::services::project_snapshot::{now_ms, write_autosave, AutosaveMeta};
    use crate::EventBus;

    fn service_in(dir: &Path) -> ProjectService {
        let store = ProjectStore::new(dir.join("library"));
        ProjectService::new(store, EventBus::new(), dir.to_path_buf())
    }

    #[tokio::test]
    async fn unsaved_project_recovery_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let svc = service_in(dir.path());
        assert!(svc.autosave_snapshot().await.is_none());
        svc.execute(ProjectCommand::Create {
            name: "Scratch".into(),
        })
        .await
        .unwrap();

        let (path, snapshot) = svc.autosave_snapshot().await.expect("snapshot");
        assert!(path.is_none(), "never saved: no path yet");
        write_autosave(
            dir.path(),
            &snapshot,
            &AutosaveMeta {
                project_path: None,
                project_name: "Scratch".into(),
                saved_at_ms: now_ms(),
            },
        )
        .unwrap();

        let found = svc.check_autosave().await.expect("offered");
        assert_eq!(found.project_name, "Scratch");
        assert!(found.project_path.is_none());

        // A fresh boot restores and consumes the copy: no second offer.
        let svc2 = service_in(dir.path());
        svc2.execute(ProjectCommand::RestoreAutosave).await.unwrap();
        assert_eq!(
            svc2.current_project().await.unwrap().meta.name,
            "Scratch"
        );
        assert!(svc2.check_autosave().await.is_none());
    }

    #[tokio::test]
    async fn stale_autosave_is_dropped_silently() {
        let dir = tempfile::tempdir().unwrap();
        let svc = service_in(dir.path());
        svc.execute(ProjectCommand::Create {
            name: "Saved".into(),
        })
        .await
        .unwrap();

        // Explicit save first…
        let save_path = dir.path().join("saved.snap");
        svc.execute(ProjectCommand::SaveAs {
            path: save_path.clone(),
            markers: vec![],
        })
        .await
        .unwrap();

        // …then a crash copy predating it (clock skew / failed clear).
        let (_, snapshot) = svc.autosave_snapshot().await.expect("snapshot");
        write_autosave(
            dir.path(),
            &snapshot,
            &AutosaveMeta {
                project_path: Some(save_path),
                project_name: "Saved".into(),
                saved_at_ms: now_ms() - 120_000,
            },
        )
        .unwrap();

        assert!(svc.check_autosave().await.is_none());
        // And the stale copy is gone, so boot stays quiet next time too.
        assert!(svc.check_autosave().await.is_none());
    }

    #[tokio::test]
    async fn shadow_copy_consumed_by_discard_or_save() {
        let dir = tempfile::tempdir().unwrap();
        let svc = service_in(dir.path());
        svc.execute(ProjectCommand::Create { name: "X".into() })
            .await
            .unwrap();
        let write_shadow = || async {
            let (_, snapshot) = svc.autosave_snapshot().await.expect("snapshot");
            write_autosave(
                dir.path(),
                &snapshot,
                &AutosaveMeta {
                    project_path: None,
                    project_name: "X".into(),
                    saved_at_ms: now_ms(),
                },
            )
            .unwrap();
        };
        write_shadow().await;
        assert!(svc.check_autosave().await.is_some());
        svc.execute(ProjectCommand::DiscardAutosave).await.unwrap();
        assert!(svc.check_autosave().await.is_none());

        // …and an explicit save consumes it the same way.
        write_shadow().await;
        svc.execute(ProjectCommand::SaveAs {
            path: dir.path().join("x.snap"),
            markers: vec![],
        })
        .await
        .unwrap();
        assert!(svc.check_autosave().await.is_none());
    }
}
