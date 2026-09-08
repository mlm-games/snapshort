use crate::jobs_service::{JobSpec, JobsService};
use crate::{
    AppError, AppEvent, AppResult, Asset, AssetCommand, AssetId, AssetStatus, AssetType, EventBus,
    ProxyPolicy, should_auto_proxy,
};
use std::collections::HashMap;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use tracing::{instrument, warn};

pub struct AssetService {
    event_bus: EventBus,
    assets: Arc<RwLock<HashMap<AssetId, Asset>>>,
    jobs: Arc<JobsService>,
    policy: Arc<RwLock<ProxyPolicy>>,
}

impl AssetService {
    pub fn new(event_bus: EventBus, jobs: Arc<JobsService>) -> Self {
        Self {
            event_bus,
            assets: Arc::new(RwLock::new(HashMap::new())),
            jobs,
            policy: Arc::new(RwLock::new(ProxyPolicy::default())),
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
            AssetCommand::SetProxyPolicy {
                auto_generate,
                min_width,
            } => {
                *self.policy.write().await = ProxyPolicy {
                    auto_generate,
                    min_width: min_width.max(1),
                };
            }
        }
        Ok(())
    }

    /// Sync an analyzed asset from the media pipeline into this map and apply
    /// the proxy policy: qualifying video auto-submits a proxy job. Called
    /// from the backend event forwarder (the job's own map is separate).
    pub async fn note_analyzed(&self, asset: Asset) {
        self.assets.write().await.insert(asset.id, asset.clone());
        if should_auto_proxy(&*self.policy.read().await, &asset) {
            if let Err(e) = self
                .jobs
                .submit(JobSpec::GenerateProxy { asset_id: asset.id })
                .await
            {
                warn!("Auto-proxy submit failed for {}: {e}", asset.id);
            }
        }
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
            let mut asset = Asset::new(path.clone(), asset_type);
            asset.status = AssetStatus::Analyzing { progress: 0 };

            let mut store = self.assets.write().await;
            store.insert(asset.id, asset.clone());
            self.event_bus.emit(AppEvent::AssetImported {
                asset: asset.clone(),
            });
            assets.push(asset.clone());

            // Sync into JobsService's asset map so the analyze job can read it
            self.jobs.insert_asset(asset.clone()).await;

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
        self.event_bus.emit(AppEvent::AssetUpdated {
            asset: asset.clone(),
        });

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
            tracing::warn!(
                "Unknown file extension '.{other}' for '{}', treating as Video",
                path.display()
            );
            AssetType::Video
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snapshort_infra_media::{MediaInfo, VideoStream};
    use snapshort_infra_store::JobStore;
    use std::path::Path;

    fn analyzed_video(width: u32) -> Asset {
        let mut asset = Asset::new(
            std::path::PathBuf::from("/tmp/does-not-exist.mp4"),
            AssetType::Video,
        );
        asset.status = AssetStatus::Ready;
        asset.media_info = Some(MediaInfo {
            container: "mp4".into(),
            duration_ms: 10_000,
            file_size: 50_000_000,
            video_streams: vec![VideoStream {
                codec_name: "h264".into(),
                codec_profile: "high".into(),
                bit_depth: Some(8),
                chroma_subsampling: Some("4:2:0".into()),
                width,
                height: 2160,
                fps: 30.0,
                duration_frames: 300,
                pixel_format: "yuv420p".into(),
                color_space: "bt709".into(),
                hdr: false,
            }],
            audio_streams: vec![],
            waveform: None,
        });
        asset
    }

    fn service_in(dir: &Path) -> (AssetService, JobStore) {
        let store = JobStore::new(dir.join("jobs"));
        let jobs = Arc::new(JobsService::new(
            store.clone(),
            EventBus::new(),
            dir.join("proxies"),
        ));
        (AssetService::new(EventBus::new(), jobs), store)
    }

    #[tokio::test]
    async fn analyzed_wide_video_upserts_and_auto_submits_proxy() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, store) = service_in(dir.path());
        let asset = analyzed_video(3840);
        let id = asset.id;
        svc.note_analyzed(asset).await;

        // Stored for snapshots/preview…
        assert!(svc.get(id).await.is_some());
        // …and a proxy job is queued (default policy: on, ≥1920px).
        assert_eq!(store.list_pending().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn narrow_video_upserts_without_proxy_job() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, store) = service_in(dir.path());
        let asset = analyzed_video(1280);
        let id = asset.id;
        svc.note_analyzed(asset).await;
        assert!(svc.get(id).await.is_some());
        assert!(store.list_pending().unwrap().is_empty());
    }

    #[tokio::test]
    async fn disabled_policy_upserts_without_proxy_job() {
        let dir = tempfile::tempdir().unwrap();
        let (svc, store) = service_in(dir.path());
        svc.execute(AssetCommand::SetProxyPolicy {
            auto_generate: false,
            min_width: 1,
        })
        .await
        .unwrap();
        svc.note_analyzed(analyzed_video(7680)).await;
        assert!(store.list_pending().unwrap().is_empty());
    }
}
