use crate::{AppEvent, EventBus};
use miniter_domain::Timestamp;
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

pub struct PlaybackService {
    event_bus: EventBus,
    state: Arc<RwLock<PlayState>>,
    current_timestamp: Arc<RwLock<Timestamp>>,
    fps: Arc<RwLock<i64>>,
    max_timestamp: Arc<RwLock<Option<Timestamp>>>,
    gen: Arc<AtomicU64>,
}

impl PlaybackService {
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            event_bus,
            state: Arc::new(RwLock::new(PlayState::Stopped)),
            current_timestamp: Arc::new(RwLock::new(Timestamp::ZERO)),
            fps: Arc::new(RwLock::new(24)),
            max_timestamp: Arc::new(RwLock::new(None)),
            gen: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn set_fps(&self, fps: i64) {
        *self.fps.write().await = fps.max(1).min(240);
    }

    pub async fn set_max_timestamp(&self, max: Option<Timestamp>) {
        *self.max_timestamp.write().await = max;
    }

    pub async fn play(&self) {
        *self.state.write().await = PlayState::Playing;
        self.event_bus.emit(AppEvent::PlaybackStarted);

        let my_gen = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        let state = self.state.clone();
        let current_ts = self.current_timestamp.clone();
        let fps = self.fps.clone();
        let max_ts = self.max_timestamp.clone();
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
                let next_ts = {
                    let mut ts = current_ts.write().await;
                    *ts = Timestamp(ts.0 + 1_000_000 / fps_val.max(1));
                    if let Some(max) = *max_ts.read().await {
                        if ts.0 >= max.0 {
                            should_stop = true;
                        }
                    }
                    *ts
                };

                event_bus.emit(AppEvent::PlayheadMoved { timestamp: next_ts });

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
        *self.current_timestamp.write().await = Timestamp::ZERO;
        self.event_bus.emit(AppEvent::PlaybackStopped);
        self.event_bus
            .emit(AppEvent::PlayheadMoved { timestamp: Timestamp::ZERO });
    }

    pub async fn seek(&self, timestamp: Timestamp) {
        let clamped = timestamp.clamp_non_negative();
        *self.current_timestamp.write().await = clamped;
        self.event_bus
            .emit(AppEvent::PlayheadMoved { timestamp: clamped });
    }

    pub async fn sync_timestamp(&self, timestamp: Timestamp) {
        *self.current_timestamp.write().await = timestamp.clamp_non_negative();
    }

    pub async fn state(&self) -> PlayState {
        *self.state.read().await
    }

    pub async fn current_timestamp(&self) -> Timestamp {
        self.current_timestamp.read().await.clamp_non_negative()
    }
}
