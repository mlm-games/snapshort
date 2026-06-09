use crate::types::AssetId;
use miniter_domain::{Timestamp, TrackId};
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
}

#[derive(Debug, Clone)]
pub enum ProjectCommand {
    Create { name: String },
    Open { path: PathBuf },
    Save { markers: Vec<crate::services::project_snapshot::TimelineMarkerData> },
    SaveAs { path: PathBuf, markers: Vec<crate::services::project_snapshot::TimelineMarkerData> },
    Close,
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
}

#[derive(Debug, Clone)]
pub enum RenderCommand {
    PreparePlan,
    Export {
        output_path: PathBuf,
        format: snapshort_infra_render::OutputFormat,
        quality: snapshort_infra_render::QualityPreset,
        use_hardware_accel: bool,
        track_volumes: std::collections::HashMap<miniter_domain::TrackId, f32>,
        master_volume: f32,
    },
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
