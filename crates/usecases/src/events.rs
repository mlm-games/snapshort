use crate::services::project_snapshot::TimelineMarkerData;
use crate::types::{Asset, AssetId};
use miniter_domain::{Project, Timeline, Timestamp};
use snapshort_infra_render::{RenderPlan, RenderResult, RenderSettings};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum AppEvent {
    // Project events
    ProjectCreated {
        project: Project,
    },
    ProjectOpened {
        project: Project,
        timeline_markers: Vec<TimelineMarkerData>,
    },
    ProjectSaved {
        path: PathBuf,
    },
    ProjectClosed,

    // Timeline events
    TimelineUpdated {
        timeline: Timeline,
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
    PlayheadMoved {
        timestamp: Timestamp,
    },

    // Preview events
    PreviewFrameReady {
        timestamp: Timestamp,
        png_bytes: Vec<u8>,
    },
    PreviewFrameFailed {
        timestamp: Timestamp,
        error: String,
    },
    TimelineThumbnailReady {
        asset_id: AssetId,
        source_time: i64,
        png_bytes: Vec<u8>,
    },
    TimelineThumbnailFailed {
        asset_id: AssetId,
        source_time: i64,
        error: String,
    },

    // Render events
    RenderPlanReady {
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

    // Jobs
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

    pub fn try_recv(&self) -> Option<AppEvent> {
        self.receiver.try_recv().ok()
    }

    pub async fn recv(&self) -> Option<AppEvent> {
        self.receiver.recv_async().await.ok()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
