//! Project service - manages project lifecycle

use crate::{AppError, AppEvent, AppResult, EventBus, ProjectCommand};
use crate::services::project_snapshot::{read_snapshot, write_snapshot, ProjectSnapshot};
use snapshort_domain::prelude::*;
use snapshort_infra_db::{
    AssetRepository, DbPool, ProjectRepository, SqliteAssetRepo, SqliteProjectRepo,
    SqliteTimelineRepo, TimelineRepository,
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, instrument};

/// Service for project operations
pub struct ProjectService {
    db: DbPool,
    project_repo: SqliteProjectRepo,
    timeline_repo: SqliteTimelineRepo,
    asset_repo: SqliteAssetRepo,
    event_bus: EventBus,

    /// Current project (in memory)
    current: Arc<RwLock<Option<Project>>>,
}

impl ProjectService {
    pub fn new(db: DbPool, event_bus: EventBus) -> Self {
        Self {
            project_repo: SqliteProjectRepo::new(db.clone()),
            timeline_repo: SqliteTimelineRepo::new(db.clone()),
            asset_repo: SqliteAssetRepo::new(db.clone()),
            db,
            event_bus,
            current: Arc::new(RwLock::new(None)),
        }
    }

    /// Get current project
    pub async fn current(&self) -> Option<Project> {
        self.current.read().await.clone()
    }

    /// Get current project ID
    pub async fn current_id(&self) -> Option<ProjectId> {
        self.current.read().await.as_ref().map(|p| p.id)
    }

    /// Execute a project command
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
            ProjectCommand::CreateTimeline { name } => {
                self.create_timeline(name).await?;
            }
            ProjectCommand::SetActiveTimeline { timeline_id } => {
                self.set_active_timeline(timeline_id).await?;
            }
        }
        Ok(())
    }

    /// Create a new project
    #[instrument(skip(self))]
    async fn create_project(&self, name: String) -> AppResult<Project> {
        let project = Project::new(&name);

        // Save to database
        self.project_repo.create(&project).await?;

        // Create default timeline
        let timeline = Timeline::new("Timeline 1").with_settings(TimelineSettings {
            fps: project.settings.fps,
            resolution: project.settings.resolution,
            sample_rate: project.settings.sample_rate,
            audio_channels: 2,
        });

        self.timeline_repo.create(project.id, &timeline).await?;

        // Update project with timeline
        let mut project = project;
        project.timeline_ids.push(timeline.id);
        project.active_timeline_id = Some(timeline.id);
        self.project_repo.update(&project).await?;

        // Set as current
        *self.current.write().await = Some(project.clone());

        self.event_bus.emit(AppEvent::ProjectCreated {
            project: project.clone(),
        });
        self.event_bus.emit(AppEvent::TimelineCreated { timeline });

        info!("Created project: {}", name);
        Ok(project)
    }

    /// Open an existing project
    #[instrument(skip(self))]
    async fn open_project(&self, path: PathBuf) -> AppResult<Project> {
        let path = normalize_project_path(path);
        let snapshot = read_snapshot(&path)?;
        self.import_snapshot(&snapshot).await?;
        let mut project = snapshot.project.clone();
        project.path = Some(path.clone());

        // Set as current
        *self.current.write().await = Some(project.clone());

        self.event_bus.emit(AppEvent::ProjectOpened {
            project: project.clone(),
        });

        info!("Opened project: {}", project.name);
        Ok(project)
    }

    /// Save current project
    #[instrument(skip(self))]
    async fn save_project(&self) -> AppResult<()> {
        let project = self
            .current
            .read()
            .await
            .clone()
            .ok_or(AppError::ProjectNotFound(uuid::Uuid::nil()))?;

        if let Some(path) = &project.path {
            let snapshot = self.snapshot_current_project().await?;
            write_snapshot(path, &snapshot)?;
            self.project_repo.update(&snapshot.project).await?;
        }

        if let Some(path) = &project.path {
            self.event_bus
                .emit(AppEvent::ProjectSaved { path: path.clone() });
        }

        info!("Saved project: {}", project.name);
        Ok(())
    }

    /// Save project to a new location
    #[instrument(skip(self))]
    async fn save_project_as(&self, path: PathBuf) -> AppResult<()> {
        let mut project = self.current.write().await;

        if let Some(ref mut p) = *project {
            let path = normalize_project_path(path);
            p.path = Some(path.clone());
            p.touch();

            let snapshot = self.snapshot_project_data(p.clone()).await?;
            write_snapshot(&path, &snapshot)?;
            p.path = Some(path.clone());
            self.project_repo.update(p).await?;
            self.event_bus.emit(AppEvent::ProjectSaved { path });

            info!("Saved project as: {}", p.name);
        }

        Ok(())
    }

    /// Close current project
    #[instrument(skip(self))]
    async fn close_project(&self) -> AppResult<()> {
        let mut current = self.current.write().await;

        if current.is_some() {
            *current = None;
            self.event_bus.emit(AppEvent::ProjectClosed);
            info!("Closed project");
        }

        Ok(())
    }

    /// Create a new timeline in current project
    #[instrument(skip(self))]
    async fn create_timeline(&self, name: String) -> AppResult<Timeline> {
        let mut project = self.current.write().await;
        let project = project
            .as_mut()
            .ok_or(AppError::ProjectNotFound(uuid::Uuid::nil()))?;

        let timeline = Timeline::new(&name).with_settings(TimelineSettings {
            fps: project.settings.fps,
            resolution: project.settings.resolution,
            sample_rate: project.settings.sample_rate,
            audio_channels: 2,
        });

        self.timeline_repo.create(project.id, &timeline).await?;

        project.timeline_ids.push(timeline.id);
        project.touch();
        self.project_repo.update(project).await?;

        self.event_bus.emit(AppEvent::TimelineCreated {
            timeline: timeline.clone(),
        });

        info!("Created timeline: {}", name);
        Ok(timeline)
    }

    /// Set active timeline
    #[instrument(skip(self))]
    async fn set_active_timeline(&self, timeline_id: TimelineId) -> AppResult<()> {
        let mut project = self.current.write().await;
        let project = project
            .as_mut()
            .ok_or(AppError::ProjectNotFound(uuid::Uuid::nil()))?;

        if !project.timeline_ids.contains(&timeline_id) {
            return Err(AppError::TimelineNotFound(timeline_id.0));
        }

        project.active_timeline_id = Some(timeline_id);
        project.touch();
        self.project_repo.update(project).await?;

        self.event_bus.emit(AppEvent::ActiveTimelineChanged {
            timeline_id: Some(timeline_id),
        });

        Ok(())
    }

    /// Get all timelines for current project
    pub async fn get_timelines(&self) -> AppResult<Vec<Timeline>> {
        let project_id = self
            .current_id()
            .await
            .ok_or(AppError::ProjectNotFound(uuid::Uuid::nil()))?;

        Ok(self.timeline_repo.get_by_project(project_id).await?)
    }

    /// Get all assets for current project
    pub async fn get_assets(&self) -> AppResult<Vec<Asset>> {
        let project_id = self
            .current_id()
            .await
            .ok_or(AppError::ProjectNotFound(uuid::Uuid::nil()))?;

        Ok(self.asset_repo.get_by_project(project_id).await?)
    }

    /// List all projects
    pub async fn list_projects(&self) -> AppResult<Vec<Project>> {
        Ok(self.project_repo.get_all().await?)
    }

    async fn snapshot_current_project(&self) -> AppResult<ProjectSnapshot> {
        let project = self
            .current
            .read()
            .await
            .clone()
            .ok_or(AppError::ProjectNotFound(uuid::Uuid::nil()))?;
        self.snapshot_project_data(project).await
    }

    async fn snapshot_project_data(&self, mut project: Project) -> AppResult<ProjectSnapshot> {
        let timelines = self.timeline_repo.get_by_project(project.id).await?;
        let assets = self.asset_repo.get_by_project(project.id).await?;
        project.timeline_ids = timelines.iter().map(|timeline| timeline.id).collect();
        project.asset_ids = assets.iter().map(|asset| asset.id).collect();
        Ok(ProjectSnapshot::new(project, assets, timelines))
    }

    async fn import_snapshot(&self, snapshot: &ProjectSnapshot) -> AppResult<()> {
        if self.project_repo.get(snapshot.project.id).await?.is_some() {
            let existing_timelines = self.timeline_repo.get_by_project(snapshot.project.id).await?;
            for timeline in existing_timelines {
                self.timeline_repo.delete(timeline.id).await?;
            }

            let existing_assets = self.asset_repo.get_by_project(snapshot.project.id).await?;
            for asset in existing_assets {
                self.asset_repo.delete(asset.id).await?;
            }

            self.project_repo.delete(snapshot.project.id).await?;
        }

        self.project_repo.create(&snapshot.project).await?;
        for timeline in &snapshot.timelines {
            self.timeline_repo.create(snapshot.project.id, timeline).await?;
        }
        for asset in &snapshot.assets {
            self.asset_repo.create(snapshot.project.id, asset).await?;
        }

        let mut project = snapshot.project.clone();
        project.timeline_ids = snapshot.timelines.iter().map(|timeline| timeline.id).collect();
        project.asset_ids = snapshot.assets.iter().map(|asset| asset.id).collect();
        self.project_repo.update(&project).await?;
        Ok(())
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

    async fn setup() -> ProjectService {
        let pool = DbPool::in_memory().await.unwrap();
        let event_bus = EventBus::new();
        ProjectService::new(pool, event_bus)
    }

    #[tokio::test]
    async fn test_create_project() {
        let service = setup().await;

        service
            .execute(ProjectCommand::Create {
                name: "Test Project".to_string(),
            })
            .await
            .unwrap();

        let project = service.current().await.unwrap();
        assert_eq!(project.name, "Test Project");
        assert_eq!(project.timeline_ids.len(), 1);
    }

    #[tokio::test]
    async fn test_create_multiple_timelines() {
        let service = setup().await;

        service
            .execute(ProjectCommand::Create {
                name: "Test".to_string(),
            })
            .await
            .unwrap();

        service
            .execute(ProjectCommand::CreateTimeline {
                name: "Timeline 2".to_string(),
            })
            .await
            .unwrap();

        let timelines = service.get_timelines().await.unwrap();
        assert_eq!(timelines.len(), 2);
    }

    #[tokio::test]
    async fn test_close_and_reopen() {
        let service = setup().await;

        service
            .execute(ProjectCommand::Create {
                name: "Test".to_string(),
            })
            .await
            .unwrap();

        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("reopen.snap");
        service
            .execute(ProjectCommand::SaveAs { path: path.clone() })
            .await
            .unwrap();

        service.execute(ProjectCommand::Close).await.unwrap();
        assert!(service.current().await.is_none());

        service
            .execute(ProjectCommand::Open { path })
            .await
            .unwrap();

        assert!(service.current().await.is_some());
    }

    #[tokio::test]
    async fn test_save_snapshot_uses_relative_asset_paths() {
        let service = setup().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let media_dir = temp_dir.path().join("media");
        std::fs::create_dir_all(&media_dir).unwrap();
        let asset_path = media_dir.join("video.mp4");
        std::fs::write(&asset_path, b"fake").unwrap();

        service
            .execute(ProjectCommand::Create {
                name: "Snapshot".to_string(),
            })
            .await
            .unwrap();

        let project_id = service.current_id().await.unwrap();
        let asset = Asset::new(asset_path.clone(), AssetType::Video);
        service.asset_repo.create(project_id, &asset).await.unwrap();

        let path = temp_dir.path().join("project.snap");
        service
            .execute(ProjectCommand::SaveAs { path: path.clone() })
            .await
            .unwrap();

        let bytes = std::fs::read_to_string(path).unwrap();
        assert!(bytes.contains("media/video.mp4"));
        assert!(!bytes.contains(&asset_path.display().to_string()));
    }
}
