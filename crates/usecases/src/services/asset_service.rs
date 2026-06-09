use crate::jobs_service::{JobSpec, JobsService};
use crate::{AppError, AppEvent, AppResult, Asset, AssetCommand, AssetId, AssetType, EventBus};
use std::collections::HashMap;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use tracing::{instrument, warn};

pub struct AssetService {
    event_bus: EventBus,
    assets: Arc<RwLock<HashMap<AssetId, Asset>>>,
    jobs: Arc<JobsService>,
}

impl AssetService {
    pub fn new(event_bus: EventBus, jobs: Arc<JobsService>) -> Self {
        Self {
            event_bus,
            assets: Arc::new(RwLock::new(HashMap::new())),
            jobs,
        }
    }

    pub async fn load_assets(&self, assets: Vec<Asset>) {
        let mut store = self.assets.write().await;
        store.clear();
        for asset in assets {
            store.insert(asset.id, asset);
        }
    }

    pub async fn list(&self) -> Vec<Asset> {
        self.assets.read().await.values().cloned().collect()
    }

    pub async fn get(&self, id: AssetId) -> Option<Asset> {
        self.assets.read().await.get(&id).cloned()
    }

    pub async fn asset_paths(&self) -> HashMap<AssetId, PathBuf> {
        self.assets
            .read()
            .await
            .iter()
            .map(|(id, asset)| (*id, asset.effective_path().clone()))
            .collect()
    }

    #[instrument(skip(self))]
    pub async fn execute(&self, command: AssetCommand) -> AppResult<()> {
        match command {
            AssetCommand::Import { paths } => {
                self.import_files(paths).await?;
            }
            AssetCommand::Analyze { asset_id } => {
                let _ = self.jobs.submit(JobSpec::AnalyzeAsset { asset_id }).await?;
            }
            AssetCommand::GenerateProxy { asset_id } => {
                let _ = self
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
        let mut assets = Vec::new();
        for path in paths {
            if !path.exists() {
                warn!("File not found: {}", path.display());
                continue;
            }

            let asset_type = detect_asset_type(&path);
            let asset = Asset::new(path.clone(), asset_type);

            let mut store = self.assets.write().await;
            store.insert(asset.id, asset.clone());
            self.event_bus.emit(AppEvent::AssetImported {
                asset: asset.clone(),
            });
            assets.push(asset.clone());

            let _ = self
                .jobs
                .submit(JobSpec::AnalyzeAsset { asset_id: asset.id })
                .await?;
        }

        Ok(assets)
    }

    #[instrument(skip(self))]
    async fn delete_asset(&self, asset_id: AssetId) -> AppResult<()> {
        let mut store = self.assets.write().await;
        if let Some(asset) = store.remove(&asset_id) {
            if let Some(proxy) = asset.proxy {
                let _ = std::fs::remove_file(proxy.path);
            }
            self.event_bus.emit(AppEvent::AssetDeleted { asset_id });
        }
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
        let mut store = self.assets.write().await;
        let Some(asset) = store.get_mut(&asset_id) else {
            return Err(AppError::AssetNotFound(asset_id.0));
        };

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
        self.event_bus
            .emit(AppEvent::AssetUpdated { asset: asset.clone() });

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
        other => {
            tracing::warn!("Unknown file extension '.{other}' for '{}', treating as Video", path.display());
            AssetType::Video
        }
    }
}
