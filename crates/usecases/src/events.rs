//! Application events (for UI updates, undo, etc.)
use snapshort_domain::prelude::*;
use snapshort_infra_render::RenderPlan;
use snapshort_infra_render::{RenderResult, RenderSettings};
use std::path::PathBuf;
use uuid::Uuid;

/// Events emitted by the application layer
#[derive(Debug, Clone)]
pub enum AppEvent {
    // Project events
    ProjectCreated {
        project: Project,
    },
    ProjectOpened {
        project: Project,
    },
    ProjectSaved {
        path: PathBuf,
    },
    ProjectClosed,

    // Timeline events
    TimelineCreated {
        timeline: Timeline,
    },
    TimelineUpdated {
        timeline: Timeline,
    },
    ActiveTimelineChanged {
        timeline_id: Option<TimelineId>,
    },
    PlayheadMoved {
        frame: Frame,
    },

    // Asset events
    AssetImported {
        asset: Asset,
    },
    AssetUpdated {
        asset: Asset,
    },
    AssetAnalyzed {
        asset: Asset,
    },
    AssetDeleted {
        asset_id: AssetId,
    },
    AssetProxyProgress {
        asset_id: AssetId,
        progress: u8,
    },
    AssetProxyComplete {
        asset: Asset,
    },

    // Bulk load (UI convenience)
    AssetsLoaded {
        assets: Vec<Asset>,
    },

    // Playback events
    PlaybackStarted,
    PlaybackPaused,
    PlaybackStopped,

    // Preview events
    PreviewFrameReady {
        frame: Frame,
        png_bytes: Vec<u8>,
    },
    PreviewFrameFailed {
        frame: Frame,
        error: String,
    },

    // Render events
    RenderPlanReady {
        timeline_id: TimelineId,
        plan: RenderPlan,
    },
    RenderStarted {
        settings: RenderSettings,
    },
    RenderFinished {
        result: RenderResult,
    },
    RenderFailed {
        error: String,
    },


    // Undo/Redo
    UndoStackChanged {
        can_undo: bool,
        can_redo: bool,
    },

    // Jobs (Phase 1)
    JobQueued {
        job_id: Uuid,
        kind: String,
    },
    JobStarted {
        job_id: Uuid,
    },
    JobProgress {
        job_id: Uuid,
        progress: u8,
        message: Option<String>,
    },
    JobFinished {
        job_id: Uuid,
    },
    JobFailed {
        job_id: Uuid,
        error: String,
    },
    JobCanceled {
        job_id: Uuid,
    },

    // Error events
    Error {
        message: String,
    },
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

    /// Receive next event (blocking async)
    pub async fn recv(&self) -> Option<AppEvent> {
        self.receiver.recv_async().await.ok()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
