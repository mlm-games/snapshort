use crate::{AppEvent, AssetId, EventBus};
use miniter_domain::{Timeline, Timestamp};
use snapshort_infra_render::RenderService;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{atomic::{AtomicU64, Ordering}, Arc};
use tokio::sync::RwLock;

pub struct PreviewService {
    event_bus: EventBus,
    renderer: Arc<RenderService>,
    timeline: Arc<RwLock<Option<Timeline>>>,
    asset_paths: Arc<RwLock<HashMap<AssetId, PathBuf>>>,
    cache: Arc<RwLock<HashMap<Timestamp, Vec<u8>>>>,
    thumbnail_cache: Arc<RwLock<HashMap<(AssetId, i64), Vec<u8>>>>,
    frame_requests_in_flight: Arc<RwLock<HashSet<Timestamp>>>,
    thumbnail_requests_in_flight: Arc<RwLock<HashSet<(AssetId, i64)>>>,
    revision: Arc<AtomicU64>,
}

const MAX_CACHE_ENTRIES: usize = 120;
const MAX_THUMBNAIL_CACHE_ENTRIES: usize = 500;

impl PreviewService {
    pub fn new(event_bus: EventBus, renderer: Arc<RenderService>) -> Self {
        Self {
            event_bus,
            renderer,
            timeline: Arc::new(RwLock::new(None)),
            asset_paths: Arc::new(RwLock::new(HashMap::new())),
            cache: Arc::new(RwLock::new(HashMap::new())),
            thumbnail_cache: Arc::new(RwLock::new(HashMap::new())),
            frame_requests_in_flight: Arc::new(RwLock::new(HashSet::new())),
            thumbnail_requests_in_flight: Arc::new(RwLock::new(HashSet::new())),
            revision: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn update_timeline(&self, timeline: Option<Timeline>) {
        *self.timeline.write().await = timeline;
        self.cache.write().await.clear();
        self.frame_requests_in_flight.write().await.clear();
        self.bump_revision();
    }

    pub async fn update_asset_paths(&self, paths: HashMap<AssetId, PathBuf>) {
        *self.asset_paths.write().await = paths;
    }

    pub async fn upsert_asset_path(&self, asset_id: AssetId, path: PathBuf) {
        self.asset_paths.write().await.insert(asset_id, path);
    }

    pub async fn remove_asset_path(&self, asset_id: AssetId) {
        self.asset_paths.write().await.remove(&asset_id);
        self.thumbnail_cache.write().await.retain(|(aid, _), _| *aid != asset_id);
        self.thumbnail_requests_in_flight.write().await.retain(|key| key.0 != asset_id);
        self.bump_revision();
    }

    pub async fn request_frame(&self, timestamp: Timestamp) {
        let Some(ref timeline) = *self.timeline.read().await else {
            return;
        };

        if let Some(bytes) = self.cache.read().await.get(&timestamp).cloned() {
            self.event_bus.emit(AppEvent::PreviewFrameReady { timestamp, png_bytes: bytes });
            return;
        }

        {
            let mut in_flight = self.frame_requests_in_flight.write().await;
            if !in_flight.insert(timestamp) {
                return;
            }
        }

        let timeline = timeline.clone();
        let renderer = self.renderer.clone();
        let cache = self.cache.clone();
        let event_bus = self.event_bus.clone();
        let in_flight = self.frame_requests_in_flight.clone();
        let requested_revision = self.current_revision();
        let revision = self.revision.clone();

        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || renderer.render_preview_frame(&timeline, timestamp))
                .await
                .map_err(|err| err.to_string())
                .and_then(|r| r.map_err(|err| err.to_string()));

            in_flight.write().await.remove(&timestamp);

            match result {
                Err(error) => {
                    event_bus.emit(AppEvent::PreviewFrameFailed { timestamp, error });
                }
                Ok(bytes) => {
                    if revision.load(Ordering::SeqCst) != requested_revision {
                        return;
                    }
                    cache.write().await.insert(timestamp, bytes.clone());
                    trim_cache_to(&mut *cache.write().await, MAX_CACHE_ENTRIES);
                    event_bus.emit(AppEvent::PreviewFrameReady { timestamp, png_bytes: bytes });
                }
            }
        });
    }

    pub async fn request_timeline_thumbnail(&self, asset_id: AssetId, source_time: i64) {
        let key = (asset_id, source_time);
        if let Some(bytes) = self.thumbnail_cache.read().await.get(&key).cloned() {
            self.event_bus.emit(AppEvent::TimelineThumbnailReady {
                asset_id,
                source_time,
                png_bytes: bytes,
            });
            return;
        }

        let source_path = {
            let paths = self.asset_paths.read().await;
            paths.get(&asset_id).cloned()
        };

        let Some(source_path) = source_path else {
            self.event_bus.emit(AppEvent::TimelineThumbnailFailed {
                asset_id,
                source_time,
                error: "Asset path not available".into(),
            });
            return;
        };

        {
            let mut in_flight = self.thumbnail_requests_in_flight.write().await;
            if !in_flight.insert(key) {
                return;
            }
        }

        let event_bus = self.event_bus.clone();
        let event_bus2 = event_bus.clone();
        let thumbnail_cache = self.thumbnail_cache.clone();
        let in_flight = self.thumbnail_requests_in_flight.clone();
        let in_flight2 = in_flight.clone();
        let requested_revision = self.current_revision();
        let revision = self.revision.clone();

        tokio::task::spawn_blocking(move || render_thumbnail_png(&source_path, source_time))
            .await
            .map_err(|err| err.to_string())
            .and_then(|result| result.map_err(|err| err.to_string()))
            .map_or_else(
                |error| {
                    tokio::spawn(async move {
                        in_flight.write().await.remove(&key);
                    });
                    event_bus.emit(AppEvent::TimelineThumbnailFailed {
                        asset_id,
                        source_time,
                        error,
                    });
                },
                |bytes| {
                    tokio::spawn(async move {
                        in_flight2.write().await.remove(&key);
                        if revision.load(Ordering::SeqCst) != requested_revision {
                            return;
                        }
                        thumbnail_cache.write().await.insert(key, bytes.clone());
                        trim_cache_to(&mut *thumbnail_cache.write().await, MAX_THUMBNAIL_CACHE_ENTRIES);
                        event_bus2.emit(AppEvent::TimelineThumbnailReady {
                            asset_id,
                            source_time,
                            png_bytes: bytes,
                        });
                    });
                },
            );
    }

    fn bump_revision(&self) {
        self.revision.fetch_add(1, Ordering::SeqCst);
    }

    fn current_revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }
}

fn trim_cache_to<K: Clone + Eq + std::hash::Hash>(cache: &mut HashMap<K, Vec<u8>>, max: usize) {
    while cache.len() > max {
        if let Some(key) = cache.keys().next().cloned() {
            cache.remove(&key);
        }
    }
}

fn render_thumbnail_png(source_path: &std::path::Path, source_time: i64) -> Result<Vec<u8>, String> {
    let output = std::process::Command::new("ffmpeg")
        .arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-ss")
        .arg(format!("{:.3}", source_time as f64 / 1_000_000.0))
        .arg("-i")
        .arg(source_path)
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
