use crate::project_snapshot::{read_snapshot, write_snapshot, ProjectSnapshot};
use crate::{AppError, AppEvent, AppResult, Asset, AssetId, EventBus, ProjectCommand};
use miniter_domain::{Project, Timeline, Timestamp};
use miniter_usecases::reducer::{dispatch, redo, undo};
use miniter_usecases::EditorState;
use snapshort_infra_db::ProjectRepo;
use snapshort_infra_db::DbConn;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, instrument};

pub struct ProjectService {
    project_repo: ProjectRepo,
    event_bus: EventBus,
    editor: Arc<RwLock<Option<EditorState>>>,
    assets: Arc<RwLock<HashMap<AssetId, Asset>>>,
    project_path: Arc<RwLock<Option<PathBuf>>>,
}

impl ProjectService {
    pub fn new(conn: DbConn, event_bus: EventBus) -> Self {
        Self {
            project_repo: ProjectRepo::new(conn),
            event_bus,
            editor: Arc::new(RwLock::new(None)),
            assets: Arc::new(RwLock::new(HashMap::new())),
            project_path: Arc::new(RwLock::new(None)),
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

    pub async fn remove_asset(&self, id: AssetId) {
        self.assets.write().await.remove(&id);
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
            ProjectCommand::Save => {
                self.save_project().await?;
            }
            ProjectCommand::SaveAs { path } => {
                self.save_project_as(path).await?;
            }
            ProjectCommand::Close => {
                self.close_project().await?;
            }
        }
        Ok(())
    }

    pub async fn dispatch_timeline_command(
        &self,
        cmd: miniter_usecases::EditCommand,
    ) -> AppResult<()> {
        let (project, can_undo, can_redo) = {
            let mut guard = self.editor.write().await;
            let mut editor = guard.take()
                .ok_or_else(|| AppError::Other("No project open".into()))?;

            dispatch(&mut editor, cmd)?;

            let result = (
                editor.project.clone(),
                editor.history.can_undo(),
                editor.history.can_redo(),
            );
            *guard = Some(editor);
            result
        };

        self.event_bus
            .emit(AppEvent::TimelineUpdated { timeline: project.timeline });
        self.event_bus.emit(AppEvent::UndoStackChanged {
            can_undo,
            can_redo,
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

        self.event_bus
            .emit(AppEvent::TimelineUpdated { timeline: project.timeline });
        self.event_bus.emit(AppEvent::UndoStackChanged {
            can_undo,
            can_redo,
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

        self.event_bus
            .emit(AppEvent::TimelineUpdated { timeline: project.timeline });
        self.event_bus.emit(AppEvent::UndoStackChanged {
            can_undo,
            can_redo,
        });

        Ok(())
    }

    pub async fn set_playhead(&self, timestamp: Timestamp) {
        let mut editor_lock = self.editor.write().await;
        if let Some(editor) = editor_lock.as_mut() {
            editor.playhead = timestamp;
        }
        self.event_bus
            .emit(AppEvent::PlayheadMoved { timestamp });
    }

    #[instrument(skip(self))]
    async fn create_project(&self, name: String) -> AppResult<Project> {
        let project = Project::new(&name);
        let editor = EditorState::new(project.clone());

        self.project_repo.create(&project).await?;
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
        let snapshot = read_snapshot(&path)?;

        let mut assets = HashMap::new();
        for asset in snapshot.assets {
            assets.insert(asset.id, asset);
        }

        let editor = EditorState::new(snapshot.project.clone());
        *self.editor.write().await = Some(editor);
        *self.assets.write().await = assets;
        *self.project_path.write().await = Some(path.clone());

        let timeline = snapshot.project.timeline.clone();
        self.event_bus.emit(AppEvent::ProjectOpened {
            project: snapshot.project.clone(),
        });
        self.event_bus
            .emit(AppEvent::AssetsLoaded { assets: self.list_assets().await });
        self.event_bus
            .emit(AppEvent::TimelineUpdated { timeline });

        info!("Opened project: {}", snapshot.project.meta.name);
        Ok(snapshot.project)
    }

    #[instrument(skip(self))]
    async fn save_project(&self) -> AppResult<()> {
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
        let snapshot = ProjectSnapshot::new(editor.project.clone(), assets);
        write_snapshot(&project_path, &snapshot)?;

        self.project_repo.create(&editor.project).await?;

        self.event_bus
            .emit(AppEvent::ProjectSaved { path: project_path });

        let project = editor.project;
        info!("Saved project: {}", project.meta.name);
        Ok(())
    }

    #[instrument(skip(self))]
    async fn save_project_as(&self, path: PathBuf) -> AppResult<()> {
        let path = normalize_project_path(path);
        let editor = self
            .editor
            .read()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| AppError::ProjectNotFound(uuid::Uuid::nil()))?;

        let assets: Vec<Asset> = self.assets.read().await.values().cloned().collect();
        let snapshot = ProjectSnapshot::new(editor.project.clone(), assets);
        write_snapshot(&path, &snapshot)?;

        *self.project_path.write().await = Some(path.clone());
        self.project_repo.create(&editor.project).await?;

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

    pub async fn list_projects(&self) -> AppResult<Vec<Project>> {
        Ok(self.project_repo.get_all().await?)
    }
}

fn normalize_project_path(path: PathBuf) -> PathBuf {
    if path.extension().is_some() {
        path
    } else {
        path.with_extension("snap")
    }
}
