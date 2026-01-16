//! Asset service - manages media assets and background processing

use crate::{AppError, AppEvent, AppResult, AssetCommand, EventBus};
use async_trait::async_trait;
use snapshort_domain::prelude::*;
use snapshort_infra_db::{AssetRepository, DbPool, SqliteAssetRepo};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, instrument, warn};

/// Trait for media analysis (implemented in infra-media)
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

/// Default no-op analyzer (replaced by real implementation)
pub struct StubAnalyzer;

#[async_trait]
impl MediaAnalyzer for StubAnalyzer {
    async fn analyze(&self, path: &PathBuf) -> AppResult<MediaInfo> {
        // Stub implementation - real one uses FFmpeg
        Ok(MediaInfo {
            container: path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("unknown")
                .to_string(),
            duration_ms: 10000,
            file_size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            video_streams: vec![VideoStream {
                codec: CodecInfo {
                    name: "h264".to_string(),
                    profile: Some("High".to_string()),
                    bit_depth: Some(8),
                    chroma_subsampling: Some("4:2:0".to_string()),
                },
                resolution: Resolution::HD,
                fps: Fps::F24,
                duration_frames: 240,
                pixel_format: "yuv420p".to_string(),
                color_space: Some("bt709".to_string()),
                hdr: false,
            }],
            audio_streams: vec![AudioStream {
                codec: CodecInfo {
                    name: "aac".to_string(),
                    profile: None,
                    bit_depth: None,
                    chroma_subsampling: None,
                },
                sample_rate: 48000,
                channels: 2,
                bit_depth: Some(16),
                duration_samples: 480000,
            }],
        })
    }

    async fn generate_proxy(
        &self,
        asset: &Asset,
        output_dir: &PathBuf,
        progress: flume::Sender<u8>,
    ) -> AppResult<ProxyInfo> {
        // Simulate progress
        for i in (0..=100).step_by(10) {
            let _ = progress.send(i);
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        Ok(ProxyInfo {
            path: output_dir.join(format!("{}_proxy.mp4", asset.id.0)),
            resolution: Resolution::new(1280, 720),
            codec: "h264".to_string(),
            created_at: chrono::Utc::now(),
        })
    }
}

/// Service for managing assets
pub struct AssetService<A: MediaAnalyzer = StubAnalyzer> {
    db: DbPool,
    asset_repo: SqliteAssetRepo,
    event_bus: EventBus,
    analyzer: Arc<A>,
    project_id: Arc<RwLock<Option<ProjectId>>>,
    proxy_dir: PathBuf,
}

impl AssetService<StubAnalyzer> {
    pub fn new(db: DbPool, event_bus: EventBus, proxy_dir: PathBuf) -> Self {
        Self::with_analyzer(db, event_bus, proxy_dir, Arc::new(StubAnalyzer))
    }
}

impl<A: MediaAnalyzer + 'static> AssetService<A> {
    pub fn with_analyzer(
        db: DbPool,
        event_bus: EventBus,
        proxy_dir: PathBuf,
        analyzer: Arc<A>,
    ) -> Self {
        Self {
            asset_repo: SqliteAssetRepo::new(db.clone()),
            db,
            event_bus,
            analyzer,
            project_id: Arc::new(RwLock::new(None)),
            proxy_dir,
        }
    }

    /// Set current project
    pub async fn set_project(&self, project_id: ProjectId) {
        *self.project_id.write().await = Some(project_id);
    }

    /// Get all assets for current project
    pub async fn list(&self) -> AppResult<Vec<Asset>> {
        let project_id = self
            .project_id
            .read()
            .await
            .ok_or(AppError::ProjectNotFound(uuid::Uuid::nil()))?;

        Ok(self.asset_repo.get_by_project(project_id).await?)
    }

    /// Get single asset
    pub async fn get(&self, id: AssetId) -> AppResult<Option<Asset>> {
        Ok(self.asset_repo.get(id).await?)
    }

    /// Execute an asset command
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

    /// Import files
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

            info!("Imported: {}", asset.name);
            self.event_bus.emit(AppEvent::AssetImported {
                asset: asset.clone(),
            });

            assets.push(asset);
        }

        // Auto-analyze imported assets
        for asset in &assets {
            let asset_id = asset.id;
            let service = self.clone_for_task();

            tokio::spawn(async move {
                if let Err(e) = service.analyze_asset(asset_id).await {
                    tracing::error!("Failed to analyze asset: {}", e);
                }
            });
        }

        Ok(assets)
    }

    /// Analyze a single asset
    #[instrument(skip(self))]
    async fn analyze_asset(&self, asset_id: AssetId) -> AppResult<()> {
        let mut asset = self
            .asset_repo
            .get(asset_id)
            .await?
            .ok_or(AppError::AssetNotFound(asset_id.0))?;

        // Update status
        asset.status = AssetStatus::Analyzing;
        self.asset_repo.update(&asset).await?;

        // Perform analysis
        match self.analyzer.analyze(&asset.path).await {
            Ok(media_info) => {
                asset.media_info = Some(media_info);
                asset.status = AssetStatus::Ready;
                asset.modified_at = chrono::Utc::now();

                self.asset_repo.update(&asset).await?;
                self.event_bus.emit(AppEvent::AssetAnalyzed { asset });

                info!("Analyzed: {}", asset_id);
            }
            Err(e) => {
                asset.status = AssetStatus::Error(e.to_string());
                self.asset_repo.update(&asset).await?;

                self.event_bus.emit(AppEvent::Error {
                    message: format!("Failed to analyze asset: {}", e),
                });
            }
        }

        Ok(())
    }

    /// Generate proxy for an asset
    #[instrument(skip(self))]
    async fn generate_proxy(&self, asset_id: AssetId) -> AppResult<()> {
        let mut asset = self
            .asset_repo
            .get(asset_id)
            .await?
            .ok_or(AppError::AssetNotFound(asset_id.0))?;

        // Create proxy directory
        std::fs::create_dir_all(&self.proxy_dir)?;

        // Progress channel
        let (tx, rx) = flume::bounded::<u8>(10);

        // Emit progress updates
        let event_bus = self.event_bus.clone();
        let progress_asset_id = asset_id;
        tokio::spawn(async move {
            while let Ok(progress) = rx.recv_async().await {
                event_bus.emit(AppEvent::AssetProxyProgress {
                    asset_id: progress_asset_id,
                    progress,
                });
            }
        });

        // Update status
        asset.status = AssetStatus::ProxyGenerating { progress: 0 };
        self.asset_repo.update(&asset).await?;

        // Generate proxy
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
                self.event_bus.emit(AppEvent::AssetProxyComplete { asset });

                info!("Proxy generated for: {}", asset_id);
            }
            Err(e) => {
                asset.status = AssetStatus::Error(e.to_string());
                self.asset_repo.update(&asset).await?;

                self.event_bus.emit(AppEvent::Error {
                    message: format!("Failed to generate proxy: {}", e),
                });
            }
        }

        Ok(())
    }

    /// Delete an asset
    #[instrument(skip(self))]
    async fn delete_asset(&self, asset_id: AssetId) -> AppResult<()> {
        // Delete proxy file if exists
        if let Some(asset) = self.asset_repo.get(asset_id).await? {
            if let Some(proxy) = &asset.proxy {
                let _ = std::fs::remove_file(&proxy.path);
            }
        }

        self.asset_repo.delete(asset_id).await?;
        self.event_bus.emit(AppEvent::AssetDeleted { asset_id });

        info!("Deleted asset: {}", asset_id);
        Ok(())
    }

    /// Update asset metadata
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

        if let Some(n) = name {
            asset.name = n;
        }
        if let Some(t) = tags {
            asset.tags = t;
        }
        if let Some(r) = rating {
            asset.rating = Some(r.min(5));
        }

        asset.modified_at = chrono::Utc::now();
        self.asset_repo.update(&asset).await?;

        self.event_bus.emit(AppEvent::AssetUpdated { asset });
        Ok(())
    }

    /// Clone for spawning tasks
    fn clone_for_task(&self) -> AssetServiceHandle {
        AssetServiceHandle {
            asset_repo: self.asset_repo.clone(),
            event_bus: self.event_bus.clone(),
            analyzer: self.analyzer.clone() as Arc<dyn MediaAnalyzer>,
            proxy_dir: self.proxy_dir.clone(),
        }
    }
}

/// Lightweight handle for async tasks
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
        self.asset_repo.update(&asset).await?;

        match self.analyzer.analyze(&asset.path).await {
            Ok(media_info) => {
                asset.media_info = Some(media_info);
                asset.status = AssetStatus::Ready;
                asset.modified_at = chrono::Utc::now();

                self.asset_repo.update(&asset).await?;
                self.event_bus.emit(AppEvent::AssetAnalyzed { asset });
            }
            Err(e) => {
                asset.status = AssetStatus::Error(e.to_string());
                self.asset_repo.update(&asset).await?;
            }
        }

        Ok(())
    }
}

/// Detect asset type from file extension
fn detect_asset_type(path: &PathBuf) -> AssetType {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        // Video
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "m4v" | "mxf" | "prores" => AssetType::Video,
        // Audio
        "mp3" | "wav" | "aac" | "flac" | "ogg" | "m4a" | "aiff" => AssetType::Audio,
        // Image
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tiff" | "webp" | "exr" | "dpx" => {
            AssetType::Image
        }
        // Default to video
        _ => AssetType::Video,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snapshort_infra_db::{ProjectRepository, SqliteProjectRepo};
    use tempfile::TempDir;

    async fn setup() -> (AssetService, ProjectId, TempDir) {
        let pool = DbPool::in_memory().await.unwrap();
        let event_bus = EventBus::new();
        let temp_dir = TempDir::new().unwrap();

        let project_repo = SqliteProjectRepo::new(pool.clone());
        let project = Project::new("Test");
        project_repo.create(&project).await.unwrap();

        let service = AssetService::new(pool, event_bus, temp_dir.path().join("proxies"));
        service.set_project(project.id).await;

        (service, project.id, temp_dir)
    }

    #[tokio::test]
    async fn test_import_and_list() {
        let (service, _, temp_dir) = setup().await;

        // Create test file
        let test_file = temp_dir.path().join("test.mp4");
        std::fs::write(&test_file, b"fake video").unwrap();

        // Import
        service
            .execute(AssetCommand::Import {
                paths: vec![test_file],
            })
            .await
            .unwrap();

        // Small delay for async analysis
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // List
        let assets = service.list().await.unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].name, "test.mp4");
    }

    #[tokio::test]
    async fn test_update_metadata() {
        let (service, _, temp_dir) = setup().await;

        let test_file = temp_dir.path().join("test.mp4");
        std::fs::write(&test_file, b"fake video").unwrap();

        service
            .execute(AssetCommand::Import {
                paths: vec![test_file],
            })
            .await
            .unwrap();

        let assets = service.list().await.unwrap();
        let asset_id = assets[0].id;

        service
            .execute(AssetCommand::UpdateMetadata {
                asset_id,
                name: Some("Renamed".to_string()),
                tags: Some(vec!["tag1".to_string()]),
                rating: Some(5),
            })
            .await
            .unwrap();

        let updated = service.get(asset_id).await.unwrap().unwrap();
        assert_eq!(updated.name, "Renamed");
        assert_eq!(updated.rating, Some(5));
    }
}
