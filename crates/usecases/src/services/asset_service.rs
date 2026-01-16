//! Asset service - manages media assets and background processing

use crate::{AppError, AppEvent, AppResult, AssetCommand, EventBus};
use async_trait::async_trait;

use snapshort_domain::{
    Asset, AssetId, AssetStatus, AssetType, AudioStream, CodecInfo, Fps, MediaInfo, ProxyInfo,
    Resolution, VideoStream,
};
use snapshort_infra_db::{
    connection::DbPool,
    repos::{asset_repo::SqliteAssetRepo, traits::AssetRepository},
};

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, instrument, warn};

/// Trait for media analysis (real implementation later)
#[async_trait]
pub trait MediaAnalyzer: Send + Sync {
    async fn analyze(&self, path: &PathBuf) -> AppResult<MediaInfo>;

    async fn generate_proxy(
        &self,
        asset: &Asset,
        output_dir: &PathBuf,
        progress: flume::Sender<u8>,
    ) -> AppResult<ProxyInfo>;
}

/// Minimal stub analyzer that always produces valid structs
#[derive(Debug, Clone, Default)]
pub struct StubAnalyzer;

fn codec(name: &str, profile: &str) -> CodecInfo {
    CodecInfo {
        name: name.to_string(),
        profile: profile.to_string(),
        bit_depth: Some(8),
        chroma_subsampling: Some("4:2:0".to_string()),
    }
}

#[async_trait]
impl MediaAnalyzer for StubAnalyzer {
    async fn analyze(&self, path: &PathBuf) -> AppResult<MediaInfo> {
        let container = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_else(|| "unknown".to_string());

        let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

        Ok(MediaInfo {
            container,
            duration_ms: 10_000,
            file_size,
            video_streams: vec![VideoStream {
                codec: codec("h264", "main"),
                resolution: Resolution::HD,
                fps: Fps::F24,
                duration_frames: 240,
                pixel_format: "yuv420p".to_string(),
                color_space: "bt709".to_string(),
                hdr: false,
            }],
            audio_streams: vec![AudioStream {
                codec: codec("aac", "lc"),
                channels: 2,
                sample_rate: 48_000,
                bit_depth: Some(16),
                duration_samples: 0,
            }],
        })
    }

    async fn generate_proxy(
        &self,
        asset: &Asset,
        output_dir: &PathBuf,
        progress: flume::Sender<u8>,
    ) -> AppResult<ProxyInfo> {
        std::fs::create_dir_all(output_dir).ok();

        // Simulate progress (fast)
        for p in (0..=100).step_by(20) {
            let _ = progress.send(p as u8);
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }

        // Create a placeholder file so downstream flow works immediately
        let proxy_path = output_dir.join(format!("{}_proxy.mp4", asset.id.0));
        let _ = std::fs::write(&proxy_path, b"proxy placeholder");

        Ok(ProxyInfo {
            path: proxy_path,
            codec: "h264".to_string(),
            bitrate_kbps: 2000,
            resolution: Resolution::HD,
            created_at: chrono::Utc::now(),
        })
    }
}

/// Service for managing assets
pub struct AssetService<A: MediaAnalyzer = StubAnalyzer> {
    db: DbPool,
    event_bus: EventBus,
    asset_repo: SqliteAssetRepo,

    /// Current project ID
    project_id: Arc<RwLock<Option<snapshort_domain::ProjectId>>>,

    analyzer: Arc<A>,
    proxy_dir: PathBuf,
}

impl AssetService<StubAnalyzer> {
    pub fn new(db: DbPool, event_bus: EventBus, proxy_dir: PathBuf) -> Self {
        Self {
            db: db.clone(),
            event_bus,
            asset_repo: SqliteAssetRepo::new(db),
            project_id: Arc::new(RwLock::new(None)),
            analyzer: Arc::new(StubAnalyzer::default()),
            proxy_dir,
        }
    }
}

impl<A: MediaAnalyzer + 'static> AssetService<A> {
    pub async fn set_project(&self, project_id: snapshort_domain::ProjectId) {
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
                self.analyze_asset(asset_id).await?;
            }
            AssetCommand::GenerateProxy { asset_id } => {
                self.generate_proxy(asset_id).await?;
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

            // Auto-analyze
            let svc = self.clone_for_task();
            let asset_id = asset.id;
            tokio::spawn(async move {
                if let Err(e) = svc.analyze_asset(asset_id).await {
                    svc.event_bus.emit(AppEvent::Error {
                        message: format!("Auto-analyze failed: {}", e),
                    });
                }
            });
        }

        Ok(assets)
    }

    #[instrument(skip(self))]
    async fn analyze_asset(&self, asset_id: AssetId) -> AppResult<()> {
        let mut asset = self
            .asset_repo
            .get(asset_id)
            .await?
            .ok_or(AppError::AssetNotFound(asset_id.0))?;

        asset.status = AssetStatus::Analyzing;
        asset.modified_at = chrono::Utc::now();
        self.asset_repo.update(&asset).await?;

        match self.analyzer.analyze(&asset.path).await {
            Ok(media_info) => {
                asset.media_info = Some(media_info);
                asset.status = AssetStatus::Ready;
                asset.modified_at = chrono::Utc::now();
                self.asset_repo.update(&asset).await?;

                self.event_bus.emit(AppEvent::AssetAnalyzed {
                    asset: asset.clone(),
                });
                info!("Analyzed asset {}", asset_id);
                Ok(())
            }
            Err(e) => {
                asset.status = AssetStatus::Error(e.to_string());
                asset.modified_at = chrono::Utc::now();
                let _ = self.asset_repo.update(&asset).await;

                self.event_bus.emit(AppEvent::Error {
                    message: format!("Analyze failed: {}", e),
                });
                Ok(())
            }
        }
    }

    #[instrument(skip(self))]
    async fn generate_proxy(&self, asset_id: AssetId) -> AppResult<()> {
        let mut asset = self
            .asset_repo
            .get(asset_id)
            .await?
            .ok_or(AppError::AssetNotFound(asset_id.0))?;

        std::fs::create_dir_all(&self.proxy_dir).ok();

        let (tx, rx): (flume::Sender<u8>, flume::Receiver<u8>) = flume::unbounded();

        // Progress forwarder
        let bus = self.event_bus.clone();
        tokio::spawn(async move {
            while let Ok(p) = rx.recv_async().await {
                bus.emit(AppEvent::AssetProxyProgress {
                    asset_id,
                    progress: p,
                });
            }
        });

        asset.status = AssetStatus::ProxyGenerating { progress: 0 };
        asset.modified_at = chrono::Utc::now();
        self.asset_repo.update(&asset).await?;

        match self
            .analyzer
            .generate_proxy(&asset, &self.proxy_dir, tx)
            .await
        {
            Ok(proxy_info) => {
                asset.proxy = Some(proxy_info);
                asset.status = AssetStatus::ProxyReady;
                asset.modified_at = chrono::Utc::now();
                self.asset_repo.update(&asset).await?;

                self.event_bus.emit(AppEvent::AssetProxyComplete {
                    asset: asset.clone(),
                });
                Ok(())
            }
            Err(e) => {
                asset.status = AssetStatus::Error(e.to_string());
                asset.modified_at = chrono::Utc::now();
                let _ = self.asset_repo.update(&asset).await;

                self.event_bus.emit(AppEvent::Error {
                    message: format!("Proxy failed: {}", e),
                });
                Ok(())
            }
        }
    }

    #[instrument(skip(self))]
    async fn delete_asset(&self, asset_id: AssetId) -> AppResult<()> {
        if let Some(asset) = self.asset_repo.get(asset_id).await? {
            if let Some(proxy) = &asset.proxy {
                let _ = std::fs::remove_file(&proxy.path);
            }
        }

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

        asset.modified_at = chrono::Utc::now();
        self.asset_repo.update(&asset).await?;
        self.event_bus.emit(AppEvent::AssetUpdated { asset });
        Ok(())
    }

    fn clone_for_task(&self) -> AssetServiceHandle {
        AssetServiceHandle {
            asset_repo: self.asset_repo.clone(),
            event_bus: self.event_bus.clone(),
            analyzer: self.analyzer.clone() as Arc<dyn MediaAnalyzer>,
            proxy_dir: self.proxy_dir.clone(),
        }
    }
}

struct AssetServiceHandle {
    asset_repo: SqliteAssetRepo,
    event_bus: EventBus,
    analyzer: Arc<dyn MediaAnalyzer>,
    proxy_dir: PathBuf,
}

impl AssetServiceHandle {
    async fn analyze_asset(&self, asset_id: AssetId) -> AppResult<()> {
        let mut asset = self
            .asset_repo
            .get(asset_id)
            .await?
            .ok_or(AppError::AssetNotFound(asset_id.0))?;

        asset.status = AssetStatus::Analyzing;
        asset.modified_at = chrono::Utc::now();
        self.asset_repo.update(&asset).await?;

        match self.analyzer.analyze(&asset.path).await {
            Ok(media_info) => {
                asset.media_info = Some(media_info);
                asset.status = AssetStatus::Ready;
                asset.modified_at = chrono::Utc::now();
                self.asset_repo.update(&asset).await?;
                self.event_bus.emit(AppEvent::AssetAnalyzed { asset });
                Ok(())
            }
            Err(e) => {
                asset.status = AssetStatus::Error(e.to_string());
                asset.modified_at = chrono::Utc::now();
                let _ = self.asset_repo.update(&asset).await;

                self.event_bus.emit(AppEvent::Error {
                    message: format!("Analyze failed: {}", e),
                });
                Ok(())
            }
        }
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
