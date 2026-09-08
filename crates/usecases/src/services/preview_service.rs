use crate::{AppEvent, AssetId, EventBus};
use miniter_domain::clip::ClipKind;
use miniter_domain::{Timeline, Timestamp};
use snapshort_infra_render::RenderService;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
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
    frames: Arc<RwLock<FrameCache>>,
    thumbnail_cache: Arc<RwLock<HashMap<(AssetId, i64), Vec<u8>>>>,
    render_slot: Arc<RenderSlot>,
    /// Source path → proxy path for preview substitution. Export always uses
    /// full-resolution sources; only the monitor consults this map.
    proxy_for_source: Arc<RwLock<HashMap<PathBuf, PathBuf>>>,
    /// When false, preview decodes originals even where proxies exist.
    prefer_proxy: Arc<AtomicBool>,
    /// While playing, the playback engine drives requests itself; the idle
    /// prefetch lane only runs paused so it never steals the render slot.
    playing: Arc<AtomicBool>,
    thumbnail_requests_in_flight: Arc<RwLock<HashSet<(AssetId, i64)>>>,
    revision: Arc<AtomicU64>,
    latest_requested: Arc<AtomicI64>,
    prev_requested: Arc<AtomicI64>,
}

/// Generation-stamped, byte-capped LRU frame cache.
///
/// Every entry carries the preview revision it was rendered under. Reads hit
/// only on the current revision, so state changes (timeline edits, asset
/// removal, proxy toggles) invalidate lazily via the revision bump instead of
/// eager full clears — and a bump-without-clear can never serve stale frames.
/// Eviction is least-recently-used under a byte cap (PNG sizes vary wildly
/// with resolution), with the old entry count kept as a backstop.
#[derive(Debug)]
struct FrameCache {
    map: HashMap<Timestamp, FrameEntry>,
    /// Front = least recently used. Refreshed on every hit and insert.
    order: VecDeque<Timestamp>,
    bytes: u64,
    max_bytes: u64,
    max_entries: usize,
}

#[derive(Debug)]
struct FrameEntry {
    revision: u64,
    bytes: Vec<u8>,
}

impl FrameCache {
    fn new(max_bytes: u64, max_entries: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            max_bytes,
            max_entries,
        }
    }

    /// Hit only when the entry belongs to the current revision. Refreshes
    /// recency so scrubbing around the playhead never evicts hot frames.
    fn get(&mut self, revision: u64, timestamp: &Timestamp) -> Option<Vec<u8>> {
        let entry = self.map.get(timestamp)?;
        if entry.revision != revision {
            return None;
        }
        if let Some(i) = self.order.iter().position(|t| t == timestamp) {
            self.order.remove(i);
            self.order.push_back(*timestamp);
        }
        Some(entry.bytes.clone())
    }

    /// Store a freshly rendered frame. Entries from older revisions are all
    /// invalid now, so they go first; then LRU eviction bounds memory. The
    /// newest write is always kept, even over cap alone — the cap is a soft
    /// budget, not a reason to drop the frame under the playhead.
    fn insert(&mut self, revision: u64, timestamp: Timestamp, bytes: Vec<u8>) {
        self.map.retain(|_, e| e.revision == revision);
        self.rebuild_order();
        if let Some(old) = self.map.remove(&timestamp) {
            self.bytes = self.bytes.saturating_sub(old.bytes.len() as u64);
        }
        self.order.retain(|t| t != &timestamp);
        self.bytes += bytes.len() as u64;
        self.map.insert(timestamp, FrameEntry { revision, bytes });
        self.order.push_back(timestamp);
        while self.map.len() > 1
            && (self.bytes > self.max_bytes || self.map.len() > self.max_entries)
        {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(removed) = self.map.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(removed.bytes.len() as u64);
            }
        }
    }

    /// Read-only freshness probe for the prefetch lane (no recency change).
    fn contains(&self, revision: u64, timestamp: &Timestamp) -> bool {
        self.map
            .get(timestamp)
            .is_some_and(|e| e.revision == revision)
    }

    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
        self.bytes = 0;
    }
    /// Rebuild recency after a retain sweep (order may reference dead keys).
    fn rebuild_order(&mut self) {
        self.order.retain(|t| self.map.contains_key(t));
        self.bytes = self.map.values().map(|e| e.bytes.len() as u64).sum();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }
}

const MAX_CACHE_ENTRIES: usize = 120;
/// Byte cap for monitor frames: 120 4K PNGs would otherwise approach a
/// gigabyte. PNG size scales with resolution, so bytes (not count) is the
/// binding constraint; the entry count stays as a backstop.
const MAX_CACHE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_THUMBNAIL_CACHE_ENTRIES: usize = 500;
/// Idle prefetch renders per quiet episode. Small on purpose: each render
/// re-checks for live requests first, so scrubbing never waits on prefetch.
const PREFETCH_PER_EPISODE: usize = 2;
/// Prefetch follows the last scrub/play direction; the step clamps here so a
/// 60s jump doesn't prefetch a minute away from the playhead.
const PREFETCH_STEP_MAX_US: i64 = 5_000_000;
/// Fallback prefetch radius with no motion history (fresh seek + pause).
const PREFETCH_DEFAULT_RADIUS_US: i64 = 1_000_000;

impl PreviewService {
    pub fn new(event_bus: EventBus, renderer: Arc<RenderService>) -> Self {
        Self {
            event_bus,
            renderer,
            timeline: Arc::new(RwLock::new(None)),
            asset_paths: Arc::new(RwLock::new(HashMap::new())),
            frames: Arc::new(RwLock::new(FrameCache::new(
                MAX_CACHE_BYTES,
                MAX_CACHE_ENTRIES,
            ))),
            thumbnail_cache: Arc::new(RwLock::new(HashMap::new())),
            render_slot: Arc::new(RenderSlot::default()),
            proxy_for_source: Arc::new(RwLock::new(HashMap::new())),
            prefer_proxy: Arc::new(AtomicBool::new(true)),
            playing: Arc::new(AtomicBool::new(false)),
            thumbnail_requests_in_flight: Arc::new(RwLock::new(HashSet::new())),
            revision: Arc::new(AtomicU64::new(0)),
            latest_requested: Arc::new(AtomicI64::new(0)),
            prev_requested: Arc::new(AtomicI64::new(0)),
        }
    }

    pub async fn update_timeline(&self, timeline: Option<Timeline>) {
        let unloaded = timeline.is_none();
        *self.timeline.write().await = timeline;
        if unloaded {
            // Project closed: no generation will ever match again, so drop
            // the frames now instead of waiting for eviction pressure.
            self.frames.write().await.clear();
        }
        // Otherwise no eager clear: entries carry their revision and miss
        // lazily after the bump below; still-valid generations survive.
        self.prune_missing_proxies().await;
        self.bump_revision();
    }

    /// Drop proxy entries whose files vanished (external delete, moved
    /// project). Runs on timeline updates — per-edit, never per-frame.
    async fn prune_missing_proxies(&self) {
        let mut map = self.proxy_for_source.write().await;
        let before = map.len();
        map.retain(|_, proxy| proxy.exists());
        if before > map.len() {
            tracing::debug!(
                "Pruned {} preview proxy mapping(s) with missing files",
                before - map.len()
            );
        }
    }

    pub async fn update_asset_paths(&self, paths: HashMap<AssetId, PathBuf>) {
        *self.asset_paths.write().await = paths;
        // Full replace means a new project: proxy mappings belong to the old one.
        self.proxy_for_source.write().await.clear();
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

    /// Register a completed proxy for preview substitution.
    pub async fn set_proxy(&self, source: PathBuf, proxy: PathBuf) {
        if proxy != source {
            self.proxy_for_source.write().await.insert(source, proxy);
        }
    }

    /// Forget the proxy for one source (asset deleted or re-probed).
    pub async fn clear_proxy_for_source(&self, source: &Path) {
        self.proxy_for_source.write().await.remove(source);
    }

    /// Switch the monitor between proxy-preferred and full-resolution
    /// decoding. Cached frames are resolution-specific, so switching retires
    /// the generation (stale entries miss lazily) and in-flight renders.
    pub async fn set_prefer_proxy(&self, prefer: bool) {
        self.prefer_proxy.store(prefer, Ordering::SeqCst);
        self.bump_revision();
    }

    /// Whether the transport is playing. The playback engine drives its own
    /// requests while playing; idle prefetch only runs paused.
    pub async fn set_playing(&self, playing: bool) {
        self.playing.store(playing, Ordering::SeqCst);
    }

    pub fn prefer_proxy(&self) -> bool {
        self.prefer_proxy.load(Ordering::SeqCst)
    }

    pub async fn request_frame(&self, timestamp: Timestamp) {
        if self.timeline.read().await.is_none() {
            return;
        }

        // Stamped hit: same revision only, so post-edit frames never serve
        // pre-edit pixels. The read refreshes recency under a write lock.
        let revision = self.current_revision();
        if let Some(bytes) = self.frames.write().await.get(revision, &timestamp) {
            self.event_bus.emit(AppEvent::PreviewFrameReady {
                timestamp,
                png_bytes: bytes,
            });
            return;
        }

        let prev = self.latest_requested.load(Ordering::SeqCst);
        self.prev_requested.store(prev, Ordering::SeqCst);
        self.latest_requested.fetch_max(timestamp.0, Ordering::SeqCst);

        // Latest wins: a running worker picks the pending timestamp up
        // itself, so only the slot winner spawns the drain loop.
        if !self.render_slot.request(timestamp) {
            return;
        }

        let timeline_cell = self.timeline.clone();
        let renderer = self.renderer.clone();
        let frames = self.frames.clone();
        let event_bus = self.event_bus.clone();
        let slot = self.render_slot.clone();
        let proxies = self.proxy_for_source.clone();
        let prefer_proxy = self.prefer_proxy.clone();
        let playing = self.playing.clone();
        let revision = self.revision.clone();
        let latest_requested = self.latest_requested.clone();
        let prev_requested = self.prev_requested.clone();

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
                let Some(ref raw) = *timeline_cell.read().await else {
                    slot.finish();
                    return;
                };
                // Proxy substitution per iteration (not per request): the map
                // may change between frames, and Full-mode bypasses it.
                let timeline = if prefer_proxy.load(Ordering::SeqCst) {
                    let proxies = proxies.read().await;
                    substitute_proxy_sources(raw, &proxies)
                } else {
                    raw.clone()
                };
                // The revision this render is based on: only a change DURING
                // the render invalidates it (a newer timeline simply renders
                // on the next drain iteration).
                let render_revision = revision.load(Ordering::SeqCst);
                let render_clone = renderer.clone();
                let result =
                    tokio::task::spawn_blocking(move || render_clone.render_preview_frame(&timeline, current))
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
                                frames
                                    .write()
                                    .await
                                    .insert(render_revision, current, bytes.clone());
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
                    // Quiet: no live demand. Prefetch likely-next frames
                    // while paused; a raced-in request resumes live work.
                    None => match prefetch_episode(
                        &timeline_cell,
                        &renderer,
                        &frames,
                        &slot,
                        &proxies,
                        &prefer_proxy,
                        &playing,
                        &revision,
                        &latest_requested,
                        &prev_requested,
                    )
                    .await
                    {
                        Some(next) => current = next,
                        None => return,
                    },
                }
            }
        });
    }

    /// Test hook: prime one frame at the current revision.
    #[cfg(test)]
    async fn prime_for_test(&self, timestamp: Timestamp, bytes: Vec<u8>) {
        let revision = self.current_revision();
        self.frames.write().await.insert(revision, timestamp, bytes);
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

/// One idle episode: render up to [`PREFETCH_PER_EPISODE`] likely-next
/// frames into the cache (never emitted — the monitor already shows the
/// requested frame). Returns a live request that raced in, if any, so the
/// drain loop resumes it instead of going quiet.
///
/// Every step re-checks for live demand, playback state, and revision drift:
/// scrubbing, playing, or editing aborts the episode immediately, and a
/// failed prefetch (e.g. newly offline media) ends it quietly without the
/// error spam a live request would deserve.
#[allow(clippy::too_many_arguments)]
async fn prefetch_episode(
    timeline_cell: &Arc<RwLock<Option<Timeline>>>,
    renderer: &Arc<RenderService>,
    frames: &Arc<RwLock<FrameCache>>,
    slot: &Arc<RenderSlot>,
    proxies: &Arc<RwLock<HashMap<PathBuf, PathBuf>>>,
    prefer_proxy: &Arc<AtomicBool>,
    playing: &Arc<AtomicBool>,
    revision: &Arc<AtomicU64>,
    latest_requested: &Arc<AtomicI64>,
    prev_requested: &Arc<AtomicI64>,
) -> Option<Timestamp> {
    if playing.load(Ordering::SeqCst) {
        return None;
    }
    let Some(ref raw) = *timeline_cell.read().await else {
        return None;
    };
    let duration_us = raw.duration_end().as_micros();
    let last = latest_requested.load(Ordering::SeqCst);
    let prev = prev_requested.load(Ordering::SeqCst);
    let targets = prefetch_targets(last, (prev != last).then_some(prev), duration_us);

    for target_us in targets.into_iter().take(PREFETCH_PER_EPISODE) {
        // Live demand always wins: yield before every render.
        if slot.has_pending() || playing.load(Ordering::SeqCst) {
            break;
        }
        let target = Timestamp::from_micros(target_us);
        if frames
            .read()
            .await
            .contains(revision.load(Ordering::SeqCst), &target)
        {
            continue;
        }
        let timeline = if prefer_proxy.load(Ordering::SeqCst) {
            let proxies = proxies.read().await;
            substitute_proxy_sources(raw, &proxies)
        } else {
            raw.clone()
        };
        let render_revision = revision.load(Ordering::SeqCst);
        let renderer = renderer.clone();
        let result =
            tokio::task::spawn_blocking(move || renderer.render_preview_frame(&timeline, target))
                .await
                .map_err(|err| err.to_string())
                .and_then(|r| r.map_err(|err| err.to_string()));
        match result {
            Ok(bytes) => {
                if revision.load(Ordering::SeqCst) != render_revision {
                    break;
                }
                frames
                    .write()
                    .await
                    .insert(render_revision, target, bytes);
            }
            // Broken source mid-episode: stop quietly, no error spam.
            Err(_) => break,
        }
    }
    slot.take_pending()
}

/// Likely-next frames around the playhead: continue the last motion
/// direction (scrub or play step), or probe both sides after a fresh seek.
/// Clamped to the timeline; the playhead itself is never a target.
fn prefetch_targets(last_us: i64, prev_us: Option<i64>, duration_us: i64) -> Vec<i64> {
    let steps: Vec<i64> = match prev_us {
        Some(prev) if prev != last_us => {
            let delta = (last_us - prev).clamp(-PREFETCH_STEP_MAX_US, PREFETCH_STEP_MAX_US);
            vec![delta, delta.saturating_mul(2)]
        }
        _ => vec![-PREFETCH_DEFAULT_RADIUS_US, PREFETCH_DEFAULT_RADIUS_US],
    };
    let mut out = Vec::with_capacity(steps.len());
    for t in steps.into_iter().map(|s| last_us.saturating_add(s)) {
        if t != last_us && t >= 0 && t <= duration_us && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// Rewrite video clip sources to their proxies where mapped. Snapshort-layer
/// substitution (not the shared encoder): preview-only, zero sibling impact.
/// Export always decodes originals.
fn substitute_proxy_sources(
    timeline: &Timeline,
    proxies: &HashMap<PathBuf, PathBuf>,
) -> Timeline {
    if proxies.is_empty() {
        return timeline.clone();
    }
    let mut out = timeline.clone();
    for track in &mut out.tracks {
        for clip in &mut track.clips {
            if let ClipKind::Video(video) = &mut clip.kind {
                if let Some(proxy) = proxies.get(&PathBuf::from(&video.source_path)) {
                    video.source_path = proxy.to_string_lossy().into_owned();
                }
            }
        }
    }
    out
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

#[cfg(test)]
mod substitution_tests {
    use super::substitute_proxy_sources;
    use miniter_domain::clip::{Clip, ClipId, ClipKind, VideoClip};
    use miniter_domain::time::{MediaDuration, Timestamp};
    use miniter_domain::track::{Track, TrackKind};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn video_clip(source: &str) -> Clip {
        Clip {
            id: ClipId(Uuid::new_v4()),
            timeline_start: Timestamp::ZERO,
            timeline_duration: MediaDuration::from_micros(1_000_000),
            source_start: MediaDuration::ZERO,
            source_end: MediaDuration::from_micros(1_000_000),
            source_total_duration: MediaDuration::from_micros(1_000_000),
            speed: 1.0,
            volume: 1.0,
            opacity: 1.0,
            muted: false,
            transition_in: None,
            transition_out: None,
            kind: ClipKind::Video(VideoClip {
                source_path: source.into(),
                width: 1920,
                height: 1080,
                fps: 30.0,
                filters: vec![],
                audio_filters: vec![],
                masks: vec![],
            }),
            keyframes: Default::default(),
            blend_mode: Default::default(),
        }
    }

    fn source_of(timeline: &miniter_domain::Timeline) -> String {
        match &timeline.tracks[0].clips[0].kind {
            ClipKind::Video(v) => v.source_path.clone(),
            _ => panic!("expected video"),
        }
    }

    fn timeline_with(clip: Clip) -> miniter_domain::Timeline {
        let mut track = Track::new(TrackKind::Video, "V1");
        track.insert_clip(clip).unwrap();
        miniter_domain::Timeline {
            tracks: vec![track],
        }
    }

    #[test]
    fn mapped_sources_rewrite_unmapped_pass_through() {
        let timeline = timeline_with(video_clip("/orig/a.mp4"));
        let mut proxies = HashMap::new();
        proxies.insert(
            PathBuf::from("/orig/a.mp4"),
            PathBuf::from("/proxy/a.mp4"),
        );
        let out = substitute_proxy_sources(&timeline, &proxies);
        assert_eq!(source_of(&out), "/proxy/a.mp4");
        // Input untouched (clone-and-rewrite, no aliasing).
        assert_eq!(source_of(&timeline), "/orig/a.mp4");

        let other = timeline_with(video_clip("/orig/b.mp4"));
        let out = substitute_proxy_sources(&other, &proxies);
        assert_eq!(source_of(&out), "/orig/b.mp4");
    }

    #[test]
    fn empty_map_clones_unchanged() {
        let timeline = timeline_with(video_clip("/orig/a.mp4"));
        let out = substitute_proxy_sources(&timeline, &HashMap::new());
        assert_eq!(source_of(&out), "/orig/a.mp4");
    }
}

#[cfg(test)]
mod frame_cache_tests {
    use super::{prefetch_targets, FrameCache};
    use miniter_domain::Timestamp;

    fn ts(us: i64) -> Timestamp {
        Timestamp::from_micros(us)
    }

    #[test]
    fn hit_only_on_current_revision() {
        let mut cache = FrameCache::new(1024 * 1024, 16);
        cache.insert(7, ts(100), vec![1, 2, 3]);
        assert_eq!(cache.get(7, &ts(100)), Some(vec![1, 2, 3]));
        // Stale generation: miss, never stale pixels.
        assert_eq!(cache.get(6, &ts(100)), None);
        assert_eq!(cache.get(8, &ts(100)), None);
        assert_eq!(cache.get(7, &ts(999)), None);
    }

    #[test]
    fn insert_evicts_older_revisions_first() {
        let mut cache = FrameCache::new(1024 * 1024, 16);
        cache.insert(7, ts(100), vec![0; 100]);
        cache.insert(7, ts(200), vec![0; 100]);
        cache.insert(8, ts(300), vec![0; 100]);
        // Both rev-7 entries are invalid now; the rev-8 write swept them.
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(8, &ts(300)), Some(vec![0; 100]));
    }

    #[test]
    fn byte_cap_evicts_least_recently_used() {
        // 300 bytes cap, 100-byte frames: room for exactly 3.
        let mut cache = FrameCache::new(300, 16);
        cache.insert(1, ts(100), vec![0; 100]);
        cache.insert(1, ts(200), vec![0; 100]);
        cache.insert(1, ts(300), vec![0; 100]);
        // Touch 100 so 200 is LRU…
        assert!(cache.get(1, &ts(100)).is_some());
        cache.insert(1, ts(400), vec![0; 100]);
        // …and 200 (not the random key, not the hot key) goes.
        assert_eq!(cache.len(), 3);
        assert!(cache.get(1, &ts(200)).is_none());
        assert!(cache.get(1, &ts(100)).is_some());
        assert!(cache.get(1, &ts(300)).is_some());
        assert!(cache.get(1, &ts(400)).is_some());
    }

    #[test]
    fn oversized_single_frame_still_stored_alone() {
        let mut cache = FrameCache::new(50, 16);
        cache.insert(1, ts(100), vec![0; 100]);
        cache.insert(1, ts(200), vec![0; 100]);
        // Cap exceeded by one frame alone: it stays (eviction loops while
        // over cap, but an empty cache keeps the single newest write).
        assert_eq!(cache.len(), 1);
        assert!(cache.get(1, &ts(200)).is_some());
    }

    #[test]
    fn reinsert_refreshes_size_accounting() {
        let mut cache = FrameCache::new(250, 16);
        cache.insert(1, ts(100), vec![0; 100]);
        cache.insert(1, ts(200), vec![0; 100]);
        // Replacing 100 with a bigger frame accounts the delta, not the sum.
        cache.insert(1, ts(100), vec![0; 150]);
        cache.insert(1, ts(300), vec![0; 50]);
        assert_eq!(cache.len(), 2);
        assert!(cache.get(1, &ts(100)).is_some());
        assert!(cache.get(1, &ts(300)).is_some());
    }

    #[test]
    fn prefetch_follows_motion_direction() {
        // Scrubbing forward in 1s steps: next two ahead.
        assert_eq!(prefetch_targets(5_000_000, Some(4_000_000), 60_000_000), vec![
            6_000_000,
            7_000_000
        ]);
        // Playing backward in 0.5s steps.
        assert_eq!(prefetch_targets(5_000_000, Some(5_500_000), 60_000_000), vec![
            4_500_000,
            4_000_000
        ]);
    }

    #[test]
    fn prefetch_without_history_probes_both_sides() {
        assert_eq!(prefetch_targets(5_000_000, None, 60_000_000), vec![
            4_000_000,
            6_000_000
        ]);
        // Same when prev == last (no direction yet).
        assert_eq!(
            prefetch_targets(5_000_000, Some(5_000_000), 60_000_000),
            vec![4_000_000, 6_000_000]
        );
    }

    #[test]
    fn prefetch_clamps_to_timeline_and_skips_playhead() {
        // Near the end heading forward: the in-bounds ahead step survives,
        // the past-the-end one drops.
        assert_eq!(
            prefetch_targets(59_000_000, Some(58_000_000), 60_000_000),
            vec![60_000_000]
        );
        // A 60s jump clamps to ±5s steps, all in bounds.
        assert_eq!(
            prefetch_targets(60_000_000, Some(0), 120_000_000),
            vec![65_000_000, 70_000_000]
        );
        // At zero heading backward: nothing valid (no negative, no self).
        assert!(prefetch_targets(0, Some(1_000_000), 60_000_000).is_empty());
    }
}

#[cfg(test)]
mod service_cache_tests {
    use super::*;
    use std::time::Duration;

    fn service() -> (PreviewService, flume::Receiver<AppEvent>) {
        let bus = EventBus::new();
        let rx = bus.receiver();
        let svc = PreviewService::new(bus, Arc::new(RenderService::new()));
        (svc, rx)
    }

    async fn next_event(rx: &flume::Receiver<AppEvent>) -> AppEvent {
        tokio::time::timeout(Duration::from_secs(5), rx.recv_async())
            .await
            .expect("timed out waiting for preview event")
            .expect("event channel closed")
    }

    #[tokio::test]
    async fn stamped_hit_serves_without_render() {
        let (svc, rx) = service();
        svc.update_timeline(Some(Timeline { tracks: vec![] }))
            .await;
        let t = Timestamp::from_micros(1_000_000);
        let primed = vec![9, 9, 9];
        svc.prime_for_test(t, primed.clone()).await;
        svc.request_frame(t).await;

        match next_event(&rx).await {
            AppEvent::PreviewFrameReady {
                timestamp,
                png_bytes,
            } => {
                assert_eq!(timestamp, t);
                // Byte-identical to the primed payload: served from cache,
                // no render ran (a real encode could never emit this).
                assert_eq!(png_bytes, primed);
            }
            other => panic!("expected cached frame, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stale_generation_misses_after_proxy_toggle() {
        let (svc, rx) = service();
        svc.update_timeline(Some(Timeline { tracks: vec![] }))
            .await;
        let t = Timestamp::from_micros(1_000_000);
        svc.prime_for_test(t, vec![9, 9, 9]).await;
        // Bumps the revision: the primed entry is stale now.
        svc.set_prefer_proxy(false).await;
        svc.request_frame(t).await;

        // The stale entry must miss: either a fresh render (different
        // bytes) or a failure — both prove no pre-toggle pixels served.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for post-toggle outcome"
            );
            match next_event(&rx).await {
                AppEvent::PreviewFrameReady { png_bytes, .. } => {
                    assert_ne!(png_bytes, vec![9, 9, 9]);
                    return;
                }
                AppEvent::PreviewFrameFailed { .. } => return,
                _ => {}
            }
        }
    }
}
