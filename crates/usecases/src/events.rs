//! Application events (for UI updates, undo, etc.)

use snapshort_domain::prelude::*;
use std::path::PathBuf;

/// Events emitted by the application layer
#[derive(Debug, Clone)]
pub enum AppEvent {
    // Project events
    ProjectCreated { project: Project },
    ProjectOpened { project: Project },
    ProjectSaved { path: PathBuf },
    ProjectClosed,

    // Timeline events
    TimelineCreated { timeline: Timeline },
    TimelineUpdated { timeline: Timeline },
    TimelineDeleted { timeline_id: TimelineId },
    ActiveTimelineChanged { timeline_id: Option<TimelineId> },
    PlayheadMoved { frame: Frame },

    // Asset events
    AssetImported { asset: Asset },
    AssetAnalyzed { asset: Asset },
    AssetProxyProgress { asset_id: AssetId, progress: u8 },
    AssetProxyComplete { asset: Asset },
    AssetDeleted { asset_id: AssetId },
    AssetUpdated { asset: Asset },

    // Playback events
    PlaybackStarted,
    PlaybackPaused,
    PlaybackStopped,

    // Undo/Redo
    UndoStackChanged { can_undo: bool, can_redo: bool },

    // Error events
    Error { message: String },
}

/// Event bus using flume channels
#[derive(Clone)]
pub struct EventBus {
    sender: flume::Sender<AppEvent>,
    receiver: flume::Receiver<AppEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, receiver) = flume::unbounded();
        Self { sender, receiver }
    }

    pub fn sender(&self) -> flume::Sender<AppEvent> {
        self.sender.clone()
    }

    pub fn receiver(&self) -> flume::Receiver<AppEvent> {
        self.receiver.clone()
    }

    pub fn emit(&self, event: AppEvent) {
        let _ = self.sender.send(event);
    }

    /// Try to receive next event (non-blocking)
    pub fn try_recv(&self) -> Option<AppEvent> {
        self.receiver.try_recv().ok()
    }

    /// Receive next event (blocking)
    pub async fn recv(&self) -> Option<AppEvent> {
        self.receiver.recv_async().await.ok()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
