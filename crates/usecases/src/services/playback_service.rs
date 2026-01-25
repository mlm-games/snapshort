//! Playback orchestration service
use crate::{AppEvent, EventBus};
use snapshort_domain::Frame;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::RwLock;
use tokio::time;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayState {
    Stopped,
    Playing,
    Paused,
}

/// Playback service - manages play/pause/stop + ticking playhead.
/// (Decode/render will come after; this completes Phase 3 “playhead playback”.)
pub struct PlaybackService {
    event_bus: EventBus,
    state: Arc<RwLock<PlayState>>,
    current_frame: Arc<RwLock<i64>>,
    fps: Arc<RwLock<i64>>,
    max_frame: Arc<RwLock<Option<i64>>>,
    // Increment to invalidate any running tick loop
    gen: Arc<AtomicU64>,
}

impl PlaybackService {
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            event_bus,
            state: Arc::new(RwLock::new(PlayState::Stopped)),
            current_frame: Arc::new(RwLock::new(0)),
            fps: Arc::new(RwLock::new(24)),
            max_frame: Arc::new(RwLock::new(None)),
            gen: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn set_fps(&self, fps: i64) {
        let fps = fps.max(1).min(240);
        *self.fps.write().await = fps;
    }

    /// If set, playback will stop when `current_frame >= max_frame`.
    pub async fn set_max_frame(&self, max: Option<i64>) {
        *self.max_frame.write().await = max.map(|m| m.max(0));
    }

    pub async fn play(&self) {
        *self.state.write().await = PlayState::Playing;
        self.event_bus.emit(AppEvent::PlaybackStarted);

        let my_gen = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        let state = self.state.clone();
        let current_frame = self.current_frame.clone();
        let fps = self.fps.clone();
        let max_frame = self.max_frame.clone();
        let gen = self.gen.clone();
        let event_bus = self.event_bus.clone();

        tokio::spawn(async move {
            loop {
                if gen.load(Ordering::SeqCst) != my_gen {
                    break;
                }
                if *state.read().await != PlayState::Playing {
                    break;
                }

                let fps_val = *fps.read().await;
                let dt = std::time::Duration::from_secs_f64(1.0 / (fps_val as f64));

                let mut should_stop = false;
                let next_frame = {
                    let mut f = current_frame.write().await;
                    *f += 1;
                    if let Some(max) = *max_frame.read().await {
                        if *f >= max {
                            should_stop = true;
                        }
                    }
                    *f
                };

                event_bus.emit(AppEvent::PlayheadMoved {
                    frame: Frame(next_frame),
                });

                if should_stop {
                    *state.write().await = PlayState::Stopped;
                    event_bus.emit(AppEvent::PlaybackStopped);
                    break;
                }

                time::sleep(dt).await;
            }
        });
    }

    pub async fn pause(&self) {
        *self.state.write().await = PlayState::Paused;
        self.gen.fetch_add(1, Ordering::SeqCst);
        self.event_bus.emit(AppEvent::PlaybackPaused);
    }

    pub async fn stop(&self) {
        *self.state.write().await = PlayState::Stopped;
        self.gen.fetch_add(1, Ordering::SeqCst);
        *self.current_frame.write().await = 0;
        self.event_bus.emit(AppEvent::PlaybackStopped);
        self.event_bus
            .emit(AppEvent::PlayheadMoved { frame: Frame(0) });
    }

    pub async fn seek(&self, frame: i64) {
        let f = frame.max(0);
        *self.current_frame.write().await = f;
        self.event_bus
            .emit(AppEvent::PlayheadMoved { frame: Frame(f) });
    }

    /// Update current frame without emitting events.
    pub async fn sync_frame(&self, frame: i64) {
        let f = frame.max(0);
        *self.current_frame.write().await = f;
    }

    pub async fn state(&self) -> PlayState {
        *self.state.read().await
    }

    pub async fn current_frame(&self) -> i64 {
        *self.current_frame.read().await
    }
}
