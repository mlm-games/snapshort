//! Asset service - manages media assets and submits background jobs
use crate::{AppError, AppEvent, AppResult, AssetCommand, EventBus};
use snapshort_domain::prelude::*;
use snapshort_infra_db::{AssetRepository, DbPool, SqliteAssetRepo};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use tracing::{instrument, warn};

// pull in JobSpec + JobsService from your jobs module
use crate::services::jobs_service::{JobSpec, JobsService};

/// Service for managing assets (CRUD + job submission)
pub struct AssetService {
    event_bus: EventBus,
    asset_repo: SqliteAssetRepo,
    /// Current project ID
    project_id: Arc<RwLock<Option<ProjectId>>>,
    /// Background job orchestrator
    jobs: Arc<JobsService>,
}

impl AssetService {
    pub fn new(db: DbPool, event_bus: EventBus, jobs: Arc<JobsService>) -> Self {
        Self {
            event_bus,
            asset_repo: SqliteAssetRepo::new(db),
            project_id: Arc::new(RwLock::new(None)),
            jobs,
        }
    }

    pub async fn set_project(&self, project_id: ProjectId) {
        *self.project_id.write().await = Some(project_id);
    }

    pub async fn list(&self) -> AppResult<Vec<Asset>> {
        let project_id = self
            .project_id
            .read()
            .await
            .ok_or(AppError::ProjectNotFound(uuid::Uuid::nil()))?;
        Ok(self.asset_repo.get_by_project(project_id).await?)
    }

    pub async fn get(&self, id: AssetId) -> AppResult<Option<Asset>> {
        Ok(self.asset_repo.get(id).await?)
    }

    #[instrument(skip(self))]
    pub async fn execute(&self, command: AssetCommand) -> AppResult<()> {
        match command {
            AssetCommand::Import { paths } => {
                self.import_files(paths).await?;
            }
            AssetCommand::Analyze { asset_id } => {
                // job-driven analyze
                let _job_id = self.jobs.submit(JobSpec::AnalyzeAsset { asset_id }).await?;
            }
            AssetCommand::GenerateProxy { asset_id } => {
                // job-driven proxy generation
                let _job_id = self
                    .jobs
                    .submit(JobSpec::GenerateProxy { asset_id })
                    .await?;
            }
            AssetCommand::Delete { asset_id } => {
                self.delete_asset(asset_id).await?;
            }
            AssetCommand::UpdateMetadata {
                asset_id,
                name,
                tags,
                rating,
            } => {
                self.update_metadata(asset_id, name, tags, rating).await?;
            }
        }
        Ok(())
    }

    #[instrument(skip(self))]
    async fn import_files(&self, paths: Vec<PathBuf>) -> AppResult<Vec<Asset>> {
        let project_id = self
            .project_id
            .read()
            .await
            .ok_or(AppError::ProjectNotFound(uuid::Uuid::nil()))?;

        let mut assets = Vec::new();
        for path in paths {
            if !path.exists() {
                warn!("File not found: {}", path.display());
                continue;
            }

            let asset_type = detect_asset_type(&path);
            let asset = Asset::new(path.clone(), asset_type);

            self.asset_repo.create(project_id, &asset).await?;
            self.event_bus.emit(AppEvent::AssetImported {
                asset: asset.clone(),
            });
            assets.push(asset.clone());

            // Auto-analyze via JobsService (no duplicate analyzer logic here)
            let _ = self
                .jobs
                .submit(JobSpec::AnalyzeAsset { asset_id: asset.id })
                .await?;
        }

        Ok(assets)
    }

    #[instrument(skip(self))]
    async fn delete_asset(&self, asset_id: AssetId) -> AppResult<()> {
        // (Optional future improvement: cancel running jobs for this asset.)
        self.asset_repo.delete(asset_id).await?;
        self.event_bus.emit(AppEvent::AssetDeleted { asset_id });
        Ok(())
    }

    #[instrument(skip(self))]
    async fn update_metadata(
        &self,
        asset_id: AssetId,
        name: Option<String>,
        tags: Option<Vec<String>>,
        rating: Option<u8>,
    ) -> AppResult<()> {
        let mut asset = self
            .asset_repo
            .get(asset_id)
            .await?
            .ok_or(AppError::AssetNotFound(asset_id.0))?;

        if let Some(name) = name {
            asset.name = name;
        }
        if let Some(tags) = tags {
            asset.tags = tags;
        }
        if let Some(r) = rating {
            asset.rating = Some(r.min(5));
        }

        asset.touch();
        self.asset_repo.update(&asset).await?;
        self.event_bus.emit(AppEvent::AssetUpdated { asset });
        Ok(())
    }
}

fn detect_asset_type(path: &PathBuf) -> AssetType {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mp4" | "mov" | "mkv" | "webm" | "avi" => AssetType::Video,
        "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" => AssetType::Audio,
        "png" | "jpg" | "jpeg" | "bmp" | "gif" | "tiff" => AssetType::Image,
        _ => AssetType::Video,
    }
}
