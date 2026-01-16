//! Playback orchestration service

use crate::{AppResult, AppEvent, EventBus};
use snapshort_domain::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio::time;
use tracing::{info, debug, error};

/// Playback state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayState {
    Stopped,
    Playing,
    Paused,
    Seeking,
}

/// Playback mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayMode {
    Forward,
    Reverse,
    Loop { start: i64, end: i64 },
}

/// Decoded frame ready for display
#[derive(Debug, Clone)]
pub struct DisplayFrame {
    pub width: u32,
    pub height: u32,
    pub pts: i64,
    pub data: Vec<u8>,
    pub timeline_frame: i64,
}

/// Playback service - manages video/audio sync
pub struct PlaybackService {
    event_bus: EventBus,

    // State
    state: Arc<RwLock<PlayState>>,
    current_frame: Arc<RwLock<i64>>,
    fps: Arc<RwLock<Fps>>,

    // Frame output channel
    frame_tx: mpsc::Sender<DisplayFrame>,
    frame_rx: Arc<RwLock<mpsc::Receiver<DisplayFrame>>>,
}

impl PlaybackService {
    pub fn new(event_bus: EventBus) -> Self {
        let (frame_tx, frame_rx) = mpsc::channel(4); // Buffer 4 frames

        Self {
            event_bus,
            state: Arc::new(RwLock::new(PlayState::Stopped)),
            current_frame: Arc::new(RwLock::new(0)),
            fps: Arc::new(RwLock::new(Fps::F24)),
            frame_tx,
            frame_rx: Arc::new(RwLock::new(frame_rx)),
        }
    }

    /// Play
    pub async fn play(&self) {
        *self.state.write().await = PlayState::Playing;
        self.event_bus.emit(AppEvent::PlaybackStarted);
    }

    /// Pause
    pub async fn pause(&self) {
        *self.state.write().await = PlayState::Paused;
        self.event_bus.emit(AppEvent::PlaybackPaused);
    }

    /// Stop
    pub async fn stop(&self) {
        *self.state.write().await = PlayState::Stopped;
        *self.current_frame.write().await = 0;
        self.event_bus.emit(AppEvent::PlaybackStopped);
    }

    /// Seek to frame
    pub async fn seek(&self, frame: i64) {
        *self.current_frame.write().await = frame;
        self.event_bus.emit(AppEvent::PlayheadMoved { frame: Frame(frame) });
    }

    /// Get current state
    pub async fn state(&self) -> PlayState {
        *self.state.read().await
    }

    /// Get current frame
    pub async fn current_frame(&self) -> i64 {
        *self.current_frame.read().await
    }

    /// Try to get next decoded frame (non-blocking)
    pub async fn try_get_frame(&self) -> Option<DisplayFrame> {
        let mut rx = self.frame_rx.write().await;
        rx.try_recv().ok()
    }
}
