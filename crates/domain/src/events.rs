//! Domain events - things that happened (for event sourcing/undo)

use crate::{AssetId, ClipId, Frame, TimelineId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DomainEvent {
    AssetImported { asset_id: AssetId },
    AssetAnalyzed { asset_id: AssetId },
    AssetProxyReady { asset_id: AssetId },
    AssetRemoved { asset_id: AssetId },

    ClipInserted { timeline_id: TimelineId, clip_id: ClipId },
    ClipRemoved { timeline_id: TimelineId, clip_id: ClipId },
    ClipMoved { timeline_id: TimelineId, clip_id: ClipId, new_start: Frame },
    ClipTrimmed { timeline_id: TimelineId, clip_id: ClipId },
    ClipSplit { timeline_id: TimelineId, clip_id: ClipId, new_clip_id: ClipId },

    TimelineCreated { timeline_id: TimelineId },
    TimelineSeek { timeline_id: TimelineId, frame: Frame },
    PlaybackStarted { timeline_id: TimelineId },
    PlaybackStopped { timeline_id: TimelineId },

    ProjectCreated,
    ProjectSaved,
    ProjectLoaded,
}
