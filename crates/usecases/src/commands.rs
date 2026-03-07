//! Command objects for operations (CQRS-light pattern)
use snapshort_domain::prelude::*;
use std::path::PathBuf;

/// Commands for timeline operations
#[derive(Debug, Clone)]
pub enum TimelineCommand {
    /// Insert a clip from an asset
    InsertClip {
        asset_id: AssetId,
        timeline_start: Frame,
        track: TrackRef,
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
        new_track: TrackRef,
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
    /// Set clip transform position (x, y)
    SetClipPosition { clip_id: ClipId, x: f32, y: f32 },
    /// Set clip scale (x, y)
    SetClipScale { clip_id: ClipId, x: f32, y: f32 },
    /// Set clip rotation (degrees)
    SetClipRotation { clip_id: ClipId, rotation: f32 },
    /// Set clip brightness
    SetClipBrightness { clip_id: ClipId, brightness: f32 },
    /// Set clip contrast
    SetClipContrast { clip_id: ClipId, contrast: f32 },
    /// Set clip saturation
    SetClipSaturation { clip_id: ClipId, saturation: f32 },
    /// Undo last operation
    Undo,
    /// Redo last undone operation
    Redo,
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

/// Phase 3: Playback commands
#[derive(Debug, Clone)]
pub enum PlaybackCommand {
    Play,
    Pause,
    Stop,
    /// Seek playhead (in frames)
    Seek {
        frame: Frame,
    },
    /// Set playback FPS (defaults to 24 if never set)
    SetFps {
        fps: i64,
    },
}

/// Preview rendering commands
#[derive(Debug, Clone)]
pub enum PreviewCommand {
    RequestFrame {
        frame: Frame,
    },
    RequestTimelineThumbnail {
        asset_id: AssetId,
        source_frame: i64,
        fps: Fps,
    },
}

/// Render/export commands
#[derive(Debug, Clone)]
pub enum RenderCommand {
    /// Build a render plan for the active timeline
    PreparePlan,
    /// Export the active timeline using the provided settings
    Export {
        output_path: std::path::PathBuf,
        format: snapshort_infra_render::OutputFormat,
        quality: snapshort_infra_render::QualityPreset,
        use_hardware_accel: bool,
    },
}
