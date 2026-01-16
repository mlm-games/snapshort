//! Command objects for operations (CQRS-light pattern)

use snapshort_domain::prelude::*;
use std::path::PathBuf;

/// Commands for timeline operations
#[derive(Debug, Clone)]
pub enum TimelineCommand {
    /// Insert a clip from an asset
    InsertClip {
        asset_id: AssetId,
        track_index: usize,
        timeline_start: Frame,
        source_range: Option<FrameRange>,
    },

    /// Remove a clip
    RemoveClip { clip_id: ClipId },

    /// Ripple delete (remove and shift following)
    RippleDelete { clip_id: ClipId },

    /// Move a clip
    MoveClip {
        clip_id: ClipId,
        new_start: Frame,
        new_track: Option<usize>,
    },

    /// Trim clip start (in-point)
    TrimStart { clip_id: ClipId, new_start: Frame },

    /// Trim clip end (out-point)
    TrimEnd { clip_id: ClipId, new_end: Frame },

    /// Split clip at playhead
    SplitAt { clip_id: ClipId, frame: Frame },

    /// Seek playhead
    Seek { frame: Frame },

    /// Add video track
    AddVideoTrack,

    /// Add audio track
    AddAudioTrack,

    /// Set clip speed
    SetClipSpeed { clip_id: ClipId, speed: f32 },

    /// Set clip opacity
    SetClipOpacity { clip_id: ClipId, opacity: f32 },
}

/// Commands for asset operations
#[derive(Debug, Clone)]
pub enum AssetCommand {
    /// Import files
    Import { paths: Vec<PathBuf> },

    /// Analyze media info
    Analyze { asset_id: AssetId },

    /// Generate proxy
    GenerateProxy { asset_id: AssetId },

    /// Delete asset
    Delete { asset_id: AssetId },

    /// Update metadata
    UpdateMetadata {
        asset_id: AssetId,
        name: Option<String>,
        tags: Option<Vec<String>>,
        rating: Option<u8>,
    },
}

/// Commands for project operations
#[derive(Debug, Clone)]
pub enum ProjectCommand {
    /// Create new project
    Create { name: String },

    /// Open existing project
    Open { path: PathBuf },

    /// Save project
    Save,

    /// Save project as
    SaveAs { path: PathBuf },

    /// Close project
    Close,

    /// Create new timeline
    CreateTimeline { name: String },

    /// Set active timeline
    SetActiveTimeline { timeline_id: TimelineId },
}
