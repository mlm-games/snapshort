use crate::{AppEvent, AssetId, EventBus};
use miniter_domain::{Timeline, Timestamp};
use snapshort_infra_render::RenderService;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicI64, AtomicU64, Ordering},
    Arc, Mutex,
};
use tokio::sync::RwLock;

/// Latest-wins render slot: at most one preview render in flight per service.
/// New requests overwrite the pending timestamp, so a scrub storm renders the
/// freshest frame instead of piling up one task per tick. All transitions
/// happen under one mutex, so a request racing worker shutdown can neither be
/// lost nor rendered twice.
#[derive(Debug, Default)]
struct RenderSlot {
    inner: Mutex<RenderSlotState>,
}

#[derive(Debug, Default)]
struct RenderSlotState {
    busy: bool,
    pending: Option<Timestamp>,
}

impl RenderSlot {
    /// Offer a timestamp. Returns true when the caller won the worker slot
    /// and must spawn the drain loop; false means the running worker will
    /// pick the (updated) pending timestamp up itself.
    fn request(&self, timestamp: Timestamp) -> bool {
        let mut state = self.inner.lock().expect("render slot poisoned");
        state.pending = Some(timestamp);
        if state.busy {
            false
        } else {
            state.busy = true;
            true
        }
    }

    /// Take the pending timestamp, keeping the worker slot.
    fn take_pending(&self) -> Option<Timestamp> {
        self.inner.lock().expect("render slot poisoned").pending.take()
    }

    /// True while a newer request arrived after the current render started.
    fn has_pending(&self) -> bool {
        self.inner.lock().expect("render slot poisoned").pending.is_some()
    }

    /// Finish the worker: hand over a raced-in pending timestamp, or release
    /// the slot when idle. Single lock op, so nothing is lost in between.
    fn finish(&self) -> Option<Timestamp> {
        let mut state = self.inner.lock().expect("render slot poisoned");
        match state.pending.take() {
            Some(timestamp) => Some(timestamp),
            None => {
                state.busy = false;
                None
            }
        }
    }
}

pub struct PreviewService {
    event_bus: EventBus,
    renderer: Arc<RenderService>,
    timeline: Arc<RwLock<Option<Timeline>>>,
    asset_paths: Arc<RwLock<HashMap<AssetId, PathBuf>>>,
    cache: Arc<RwLock<HashMap<Timestamp, Vec<u8>>>>,
    thumbnail_cache: Arc<RwLock<HashMap<(AssetId, i64), Vec<u8>>>>,
    render_slot: Arc<RenderSlot>,
    thumbnail_requests_in_flight: Arc<RwLock<HashSet<(AssetId, i64)>>>,
    revision: Arc<AtomicU64>,
    latest_requested: Arc<AtomicI64>,
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
            render_slot: Arc::new(RenderSlot::default()),
            thumbnail_requests_in_flight: Arc::new(RwLock::new(HashSet::new())),
            revision: Arc::new(AtomicU64::new(0)),
            latest_requested: Arc::new(AtomicI64::new(0)),
        }
    }

    pub async fn update_timeline(&self, timeline: Option<Timeline>) {
        *self.timeline.write().await = timeline;
        self.cache.write().await.clear();
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
        if self.timeline.read().await.is_none() {
            return;
        }

        if let Some(bytes) = self.cache.read().await.get(&timestamp).cloned() {
            self.event_bus.emit(AppEvent::PreviewFrameReady {
                timestamp,
                png_bytes: bytes,
            });
            return;
        }

        self.latest_requested.fetch_max(timestamp.0, Ordering::SeqCst);

        // Latest wins: a running worker picks the pending timestamp up
        // itself, so only the slot winner spawns the drain loop.
        if !self.render_slot.request(timestamp) {
            return;
        }

        let timeline_cell = self.timeline.clone();
        let renderer = self.renderer.clone();
        let cache = self.cache.clone();
        let event_bus = self.event_bus.clone();
        let slot = self.render_slot.clone();
        let revision = self.revision.clone();
        let latest_requested = self.latest_requested.clone();

        tokio::spawn(async move {
            // Drain loop: always render the freshest pending frame. Superseded
            // timestamps never render — the worker skips straight to latest.
            let mut current = match slot.take_pending() {
                Some(timestamp) => timestamp,
                None => {
                    slot.finish();
                    return;
                }
            };
            loop {
                let Some(ref timeline) = *timeline_cell.read().await else {
                    slot.finish();
                    return;
                };
                let timeline = timeline.clone();
                // The revision this render is based on: only a change DURING
                // the render invalidates it (a newer timeline simply renders
                // on the next drain iteration).
                let render_revision = revision.load(Ordering::SeqCst);
                let renderer = renderer.clone();
                let result =
                    tokio::task::spawn_blocking(move || renderer.render_preview_frame(&timeline, current))
                        .await
                        .map_err(|err| err.to_string())
                        .and_then(|r| r.map_err(|err| err.to_string()));

                match result {
                    Err(error) => {
                        event_bus.emit(AppEvent::PreviewFrameFailed {
                            timestamp: current,
                            error,
                        });
                    }
                    Ok(bytes) => {
                        // Drop late completions: the timeline moved on mid-render,
                        // or a fresher frame is already pending (emitting it would
                        // flicker the monitor backwards). The pending frame
                        // renders next in this same loop.
                        let superseded = revision.load(Ordering::SeqCst) != render_revision
                            || slot.has_pending();
                        if !superseded {
                            let latest = latest_requested.load(Ordering::SeqCst);
                            if current.0 >= latest - 500_000 {
                                let mut cache = cache.write().await;
                                cache.insert(current, bytes.clone());
                                trim_cache_to(&mut cache, MAX_CACHE_ENTRIES);
                                event_bus.emit(AppEvent::PreviewFrameReady {
                                    timestamp: current,
                                    png_bytes: bytes,
                                });
                            }
                        }
                    }
                }

                match slot.finish() {
                    Some(next) => current = next,
                    None => return,
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
        let renderer = self.renderer.clone();

        tokio::task::spawn_blocking(move || render_thumbnail_png(&renderer, &source_path, source_time))
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
                        let mut cache = thumbnail_cache.write().await;
                        cache.insert(key, bytes.clone());
                        trim_cache_to(&mut cache, MAX_THUMBNAIL_CACHE_ENTRIES);
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

fn render_thumbnail_png(renderer: &RenderService, source_path: &std::path::Path, source_time: i64) -> Result<Vec<u8>, String> {
    renderer
        .render_thumbnail(&source_path.display().to_string(), source_time)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod slot_tests {
    use super::RenderSlot;
    use miniter_domain::Timestamp;

    #[test]
    fn slot_serializes_latest_wins() {
        let slot = RenderSlot::default();
        // First request wins the worker slot; the second queues as pending…
        assert!(slot.request(Timestamp::from_micros(100)));
        assert!(!slot.request(Timestamp::from_micros(200)));
        // …and pending always holds the latest request.
        assert_eq!(slot.take_pending(), Some(Timestamp::from_micros(200)));

        // A request racing worker shutdown is handed over without releasing…
        assert!(!slot.request(Timestamp::from_micros(300)));
        assert_eq!(slot.finish(), Some(Timestamp::from_micros(300)));
        // …and a drained slot releases for the next worker.
        assert_eq!(slot.finish(), None);
        assert!(slot.request(Timestamp::from_micros(400)));
        assert_eq!(slot.take_pending(), Some(Timestamp::from_micros(400)));
        assert_eq!(slot.finish(), None);
    }

    #[test]
    fn has_pending_tracks_supersede() {
        let slot = RenderSlot::default();
        assert!(!slot.has_pending());
        slot.request(Timestamp::from_micros(100));
        assert!(slot.has_pending());
        slot.take_pending();
        assert!(!slot.has_pending());
    }
}
