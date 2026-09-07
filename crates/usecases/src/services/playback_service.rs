use crate::time::FrameStepper;
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
    stepper: Arc<RwLock<FrameStepper>>,
    max_timestamp: Arc<RwLock<Option<Timestamp>>>,
    generation: Arc<AtomicU64>,
}

impl PlaybackService {
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            event_bus,
            state: Arc::new(RwLock::new(PlayState::Stopped)),
            current_timestamp: Arc::new(RwLock::new(Timestamp::ZERO)),
            fps: Arc::new(RwLock::new(24)),
            stepper: Arc::new(RwLock::new(FrameStepper::new(24))),
            max_timestamp: Arc::new(RwLock::new(None)),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn set_fps(&self, fps: i64) {
        let fps = fps.max(1).min(240);
        *self.fps.write().await = fps;
        self.stepper.write().await.set_fps(fps);
    }

    pub async fn set_max_timestamp(&self, max: Option<Timestamp>) {
        *self.max_timestamp.write().await = max;
    }

    pub async fn play(&self) {
        *self.state.write().await = PlayState::Playing;
        self.event_bus.emit(AppEvent::PlaybackStarted);

        let my_gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let state = self.state.clone();
        let current_ts = self.current_timestamp.clone();
        let fps = self.fps.clone();
        let stepper = self.stepper.clone();
        let max_ts = self.max_timestamp.clone();
        let generation = self.generation.clone();
        let event_bus = self.event_bus.clone();

        tokio::spawn(async move {
            let mut dropped_total: u32 = 0;
            let mut expected = web_time::Instant::now();
            loop {
                if generation.load(Ordering::SeqCst) != my_gen {
                    break;
                }
                if *state.read().await != PlayState::Playing {
                    break;
                }

                let fps_val = (*fps.read().await).max(1);
                let frame_us = 1_000_000 / fps_val;
                expected += web_time::Duration::from_micros(frame_us as u64);

                // Catch up by skipping instead of bursting: when the loop
                // falls behind (slow UI pump, heavy timeline), advance whole
                // missed frames at once so the monitor always shows the
                // freshest timestamp. Capped per iteration to bound one stall.
                let mut steps: i64 = 1;
                let now = web_time::Instant::now();
                if now > expected {
                    let behind_us = now.duration_since(expected).as_micros() as i64;
                    let missed = (behind_us / frame_us).min(600);
                    if missed > 0 {
                        steps += missed;
                        dropped_total = dropped_total.saturating_add(missed as u32);
                        tracing::debug!("playback behind by {missed} frames; skipping ahead");
                    }
                    expected = now;
                }

                let mut should_stop = false;
                let next_ts = {
                    let mut ts = current_ts.write().await;
                    let mut stepper = stepper.write().await;
                    for _ in 0..steps {
                        *ts = Timestamp(ts.0 + stepper.next_step_us());
                    }
                    if let Some(max) = *max_ts.read().await {
                        if ts.0 >= max.0 {
                            should_stop = true;
                        }
                    }
                    *ts
                };

                event_bus.emit(AppEvent::PlayheadMoved {
                    timestamp: next_ts,
                    dropped_total,
                });

                if should_stop {
                    *state.write().await = PlayState::Stopped;
                    event_bus.emit(AppEvent::PlaybackStopped);
                    break;
                }

                let now = web_time::Instant::now();
                if expected > now {
                    time::sleep(expected - now).await;
                }
                // Already behind: loop immediately; catch-up runs next round.
            }
        });
    }

    pub async fn pause(&self) {
        *self.state.write().await = PlayState::Paused;
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.event_bus.emit(AppEvent::PlaybackPaused);
    }

    pub async fn stop(&self) {
        *self.state.write().await = PlayState::Stopped;
        self.generation.fetch_add(1, Ordering::SeqCst);
        *self.current_timestamp.write().await = Timestamp::ZERO;
        self.stepper.write().await.reset();
        self.event_bus.emit(AppEvent::PlaybackStopped);
        self.event_bus.emit(AppEvent::PlayheadMoved {
            timestamp: Timestamp::ZERO,
            dropped_total: 0,
        });
    }

    pub async fn seek(&self, timestamp: Timestamp) {
        let clamped = timestamp.clamp_non_negative();
        *self.current_timestamp.write().await = clamped;
        self.stepper.write().await.reset();
        self.event_bus.emit(AppEvent::PlayheadMoved {
            timestamp: clamped,
            dropped_total: 0,
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppEvent, EventBus};

    fn drain_moves(rx: &flume::Receiver<AppEvent>) -> (u32, i64, u32) {
        let mut count = 0u32;
        let mut last_ts = 0i64;
        let mut dropped = 0u32;
        while let Ok(ev) = rx.try_recv() {
            if let AppEvent::PlayheadMoved {
                timestamp,
                dropped_total,
            } = ev
            {
                assert!(
                    timestamp.0 >= last_ts,
                    "playhead went backwards: {} -> {}",
                    last_ts,
                    timestamp.0
                );
                last_ts = timestamp.0;
                count += 1;
                dropped = dropped_total;
            }
        }
        (count, last_ts, dropped)
    }

    #[tokio::test]
    async fn ticker_advances_monotonically_and_stops_on_pause() {
        let bus = EventBus::new();
        let rx = bus.receiver();
        let svc = PlaybackService::new(bus);
        svc.set_fps(240).await;
        svc.play().await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        svc.pause().await;

        let (count, last_ts, _) = drain_moves(&rx);
        // 240fps × 0.3s ≈ 72 ticks; wide bound for loaded CI runners.
        assert!(count > 10, "ticker emitted only {count} ticks");
        assert!(last_ts > 0, "playhead never advanced");

        // Paused: the loop exits within one frame period; nothing new arrives.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let (extra, _, _) = drain_moves(&rx);
        assert_eq!(extra, 0, "ticker kept emitting after pause");
        assert_eq!(svc.state().await, PlayState::Paused);
    }

    #[tokio::test]
    async fn stop_resets_to_zero() {
        let bus = EventBus::new();
        let svc = PlaybackService::new(bus);
        svc.play().await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        svc.stop().await;
        assert_eq!(svc.current_timestamp().await, Timestamp::ZERO);
        assert_eq!(svc.state().await, PlayState::Stopped);
    }
}
