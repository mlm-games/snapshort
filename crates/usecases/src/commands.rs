use crate::formats::{OutputFormat, QualityPreset};
use crate::types::AssetId;
use miniter_domain::Timestamp;
use miniter_usecases::EditCommand;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum AssetCommand {
    Import { paths: Vec<PathBuf> },
    Analyze { asset_id: AssetId },
    GenerateProxy { asset_id: AssetId },
    Delete { asset_id: AssetId },
    UpdateMetadata {
        asset_id: AssetId,
        name: Option<String>,
        tags: Option<Vec<String>>,
        rating: Option<u8>,
    },
    SetProxyPolicy {
        auto_generate: bool,
        min_width: u32,
    },
    /// Point an offline asset at a new file. The stale proxy (if any) is
    /// discarded, the asset re-analyzes, and other offline assets whose
    /// filenames exist next to the new file are relinked automatically
    /// (Resolve-style "relink others").
    Relink {
        asset_id: AssetId,
        new_path: PathBuf,
    },
    /// Relink every offline asset whose filename exists under `dir`.
    RelinkInFolder { dir: PathBuf },
}

#[derive(Debug, Clone)]
pub enum ProjectCommand {
    Create { name: String },
    Open { path: PathBuf },
    Save { markers: Vec<crate::types::TimelineMarkerData> },
    SaveAs { path: PathBuf, markers: Vec<crate::types::TimelineMarkerData> },
    Close,
    /// Restore the crash-recovery shadow copy (recovery prompt).
    RestoreAutosave,
    /// Delete the shadow copy without restoring (recovery prompt).
    DiscardAutosave,
}

#[derive(Debug, Clone)]
pub enum PlaybackCommand {
    Play,
    Pause,
    Stop,
    Seek { timestamp: Timestamp },
    SetFps { fps: i64 },
}

#[derive(Debug, Clone)]
pub enum PreviewCommand {
    RequestFrame { timestamp: Timestamp },
    RequestTimelineThumbnail {
        asset_id: AssetId,
        source_time: i64,
    },
    /// Monitor quality: proxy-preferred decoding (default) or full-resolution
    /// originals. Export always uses originals regardless.
    SetPreferProxy { prefer: bool },
}

#[derive(Debug, Clone)]
pub enum RenderCommand {
    PreparePlan,
    Export {
        output_path: PathBuf,
        format: OutputFormat,
        quality: QualityPreset,
        use_hardware_accel: bool,
        track_volumes: std::collections::HashMap<miniter_domain::TrackId, f32>,
        master_volume: f32,
        /// Caller overrides layered over the recommended settings; an export
        /// preset's shape arrives this way.
        overrides: RenderOverrides,
    },
    /// Signal a running export to stop at the next encoder checkpoint.
    CancelExport,
}

/// Optional export-setting overrides; `None` keeps the recommended value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderOverrides {
    pub resolution: Option<(u32, u32)>,
    pub fps: Option<f64>,
    pub video_bitrate_kbps: Option<u32>,
    pub audio_bitrate_kbps: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum AppCommand {
    Edit(EditCommand),
    Asset(AssetCommand),
    Project(ProjectCommand),
    Playback(PlaybackCommand),
    Preview(PreviewCommand),
    Render(RenderCommand),
    Undo,
    Redo,
}

/// The command the app speaks, on every platform: pure data, no services.
///
/// The UI sends these over a channel; the CLI and the MCP server send the JSON
/// form of them. Keeping the type free of the session means the web shell and
/// the desktop shell share one command vocabulary.
#[derive(Debug, Clone)]
pub enum SessionCommand {
    Project(ProjectCommand),
    /// An edit plus its undo label; a non-empty label folds consecutive
    /// same-label edits (slider ticks, keystrokes) into one undo step.
    Edit {
        cmd: Box<EditCommand>,
        label: String,
    },
    Asset(AssetCommand),
    Playback(PlaybackCommand),
    Preview(PreviewCommand),
    Render(RenderCommand),
    /// The editor selection (clips and/or tracks).
    Select {
        clips: Vec<miniter_domain::ClipId>,
        tracks: Vec<miniter_domain::TrackId>,
    },
    /// A JSON command, for UI actions that are engine commands at heart.
    RunJson {
        id: String,
        params: serde_json::Value,
    },
    Undo,
    Redo,
}
