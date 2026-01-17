//! Domain events - pure (no flume, no app event bus)

use crate::{AssetId, ClipId, Frame, ProjectId, TimelineId};

#[derive(Debug, Clone)]
pub enum DomainEvent {
    // Project
    ProjectCreated {
        project_id: ProjectId,
    },
    ProjectRenamed {
        project_id: ProjectId,
        name: String,
    },

    // Timeline
    TimelineCreated {
        timeline_id: TimelineId,
        project_id: ProjectId,
    },
    ActiveTimelineChanged {
        project_id: ProjectId,
        timeline_id: Option<TimelineId>,
    },
    PlayheadMoved {
        timeline_id: TimelineId,
        frame: Frame,
    },

    // Assets
    AssetImported {
        project_id: ProjectId,
        asset_id: AssetId,
    },
    AssetAnalyzed {
        asset_id: AssetId,
    },
    AssetProxyProgress {
        asset_id: AssetId,
        progress: u8,
    },
    AssetProxyReady {
        asset_id: AssetId,
    },

    // Clips
    ClipInserted {
        timeline_id: TimelineId,
        clip_id: ClipId,
    },
    ClipRemoved {
        timeline_id: TimelineId,
        clip_id: ClipId,
    },
}
