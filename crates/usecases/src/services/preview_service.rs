use crate::{AppEvent, EventBus};
use snapshort_domain::prelude::*;
use snapshort_infra_render::RenderService;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct PreviewService {
    event_bus: EventBus,
    renderer: Arc<RenderService>,
    timeline: Arc<RwLock<Option<Timeline>>>,
    assets: Arc<RwLock<HashMap<AssetId, Asset>>>,
    cache: Arc<RwLock<HashMap<(TimelineId, i64), Vec<u8>>>>,
    thumbnail_cache: Arc<RwLock<HashMap<(AssetId, i64), Vec<u8>>>>,
}

impl PreviewService {
    pub fn new(event_bus: EventBus, renderer: Arc<RenderService>) -> Self {
        Self {
            event_bus,
            renderer,
            timeline: Arc::new(RwLock::new(None)),
            assets: Arc::new(RwLock::new(HashMap::new())),
            cache: Arc::new(RwLock::new(HashMap::new())),
            thumbnail_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn update_timeline(&self, timeline: Option<Timeline>) {
        *self.timeline.write().await = timeline;
        self.cache.write().await.clear();
    }

    pub async fn update_assets(&self, assets: Vec<Asset>) {
        *self.assets.write().await = assets.into_iter().map(|asset| (asset.id, asset)).collect();
        self.cache.write().await.clear();
        self.thumbnail_cache.write().await.clear();
    }

    pub async fn upsert_asset(&self, asset: Asset) {
        self.assets.write().await.insert(asset.id, asset);
        self.cache.write().await.clear();
        self.thumbnail_cache.write().await.clear();
    }

    pub async fn remove_asset(&self, asset_id: AssetId) {
        self.assets.write().await.remove(&asset_id);
        self.cache.write().await.clear();
        self.thumbnail_cache.write().await.clear();
    }

    pub async fn request_frame(&self, frame: Frame) {
        let Some(timeline) = self.timeline.read().await.clone() else {
            return;
        };

        let key = (timeline.id, frame.0);
        if let Some(bytes) = self.cache.read().await.get(&key).cloned() {
            self.event_bus.emit(AppEvent::PreviewFrameReady { frame, png_bytes: bytes });
            return;
        }

        let assets: Vec<_> = self.assets.read().await.values().cloned().collect();
        let renderer = self.renderer.clone();
        let cache = self.cache.clone();
        let event_bus = self.event_bus.clone();

        tokio::task::spawn_blocking(move || renderer.render_preview_frame(&timeline, &assets, frame))
            .await
            .map_err(|err| err.to_string())
            .and_then(|result| result.map_err(|err| err.to_string()))
            .map_or_else(
                |error| {
                    event_bus.emit(AppEvent::PreviewFrameFailed { frame, error });
                },
                |bytes| {
                    let cache = cache.clone();
                    let event_bus = event_bus.clone();
                    tokio::spawn(async move {
                        cache.write().await.insert(key, bytes.clone());
                        event_bus.emit(AppEvent::PreviewFrameReady { frame, png_bytes: bytes });
                    });
                },
            );
    }

    pub async fn request_timeline_thumbnail(&self, asset_id: AssetId, source_frame: i64, fps: Fps) {
        let key = (asset_id, source_frame);
        if let Some(bytes) = self.thumbnail_cache.read().await.get(&key).cloned() {
            self.event_bus.emit(AppEvent::TimelineThumbnailReady {
                asset_id,
                source_frame,
                png_bytes: bytes,
            });
            return;
        }

        let asset = self.assets.read().await.get(&asset_id).cloned();
        let Some(asset) = asset else {
            self.event_bus.emit(AppEvent::TimelineThumbnailFailed {
                asset_id,
                source_frame,
                error: "Asset not available for thumbnail generation".into(),
            });
            return;
        };

        let event_bus = self.event_bus.clone();
        let thumbnail_cache = self.thumbnail_cache.clone();

        tokio::task::spawn_blocking(move || render_thumbnail_png(asset, source_frame, fps))
            .await
            .map_err(|err| err.to_string())
            .and_then(|result| result.map_err(|err| err.to_string()))
            .map_or_else(
                |error| {
                    event_bus.emit(AppEvent::TimelineThumbnailFailed {
                        asset_id,
                        source_frame,
                        error,
                    });
                },
                |bytes| {
                    let event_bus = event_bus.clone();
                    let thumbnail_cache = thumbnail_cache.clone();
                    tokio::spawn(async move {
                        thumbnail_cache.write().await.insert(key, bytes.clone());
                        event_bus.emit(AppEvent::TimelineThumbnailReady {
                            asset_id,
                            source_frame,
                            png_bytes: bytes,
                        });
                    });
                },
            );
    }
}

fn render_thumbnail_png(asset: Asset, source_frame: i64, fps: Fps) -> Result<Vec<u8>, String> {
    let output = std::process::Command::new("ffmpeg")
        .arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-ss")
        .arg(format!("{:.3}", source_frame as f64 / fps.as_f64().max(0.001)))
        .arg("-i")
        .arg(asset.effective_path())
        .arg("-vframes")
        .arg("1")
        .arg("-vf")
        .arg("scale=160:90:flags=lanczos")
        .arg("-f")
        .arg("image2")
        .arg("-")
        .output()
        .map_err(|err| err.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    Ok(output.stdout)
}
