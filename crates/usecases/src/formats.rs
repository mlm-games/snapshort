//! Output formats and render settings: pure data shared by the shells, the
//! control channel and the native renderer.

use std::path::PathBuf;

/// Every variant maps to a container and codec the shared miniter exporter can
/// actually write. Formats the exporter cannot produce are not offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Mp4H264,
    H264Mkv,
    MovH264,
    Av1Mp4,
    Av1Mkv,
    Av1WebM,
    Av1Ivf,
    Opus,
    Flac,
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self::Mp4H264
    }
}

impl OutputFormat {
    /// The only formats with no video stream; their video settings are unused.
    pub fn is_audio_only(self) -> bool {
        matches!(self, Self::Opus | Self::Flac)
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4H264 | Self::Av1Mp4 => "mp4",
            Self::H264Mkv | Self::Av1Mkv => "mkv",
            Self::MovH264 => "mov",
            Self::Av1WebM => "webm",
            Self::Av1Ivf => "ivf",
            Self::Opus => "opus",
            Self::Flac => "flac",
        }
    }

    /// The canonical name the CLI and automation accept; extensions and
    /// aliases are resolved on top of this.
    pub fn name(self) -> &'static str {
        match self {
            Self::Mp4H264 => "mp4",
            Self::H264Mkv => "mkv",
            Self::MovH264 => "mov",
            Self::Av1Mp4 => "av1-mp4",
            Self::Av1Mkv => "av1-mkv",
            Self::Av1WebM => "av1-webm",
            Self::Av1Ivf => "av1-ivf",
            Self::Opus => "opus",
            Self::Flac => "flac",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mp4H264 => "H.264 / MP4",
            Self::H264Mkv => "H.264 / MKV",
            Self::MovH264 => "H.264 / MOV",
            Self::Av1Mp4 => "AV1 / MP4",
            Self::Av1Mkv => "AV1 / MKV",
            Self::Av1WebM => "AV1 / WebM",
            Self::Av1Ivf => "AV1 / IVF",
            Self::Opus => "Opus / Ogg",
            Self::Flac => "FLAC",
        }
    }

    pub fn export_format(self) -> miniter_domain::export::ExportFormat {
        use miniter_domain::export::ExportFormat;
        match self {
            Self::Mp4H264 => ExportFormat::Mp4,
            Self::H264Mkv => ExportFormat::H264Mkv,
            Self::MovH264 => ExportFormat::Mov,
            Self::Av1Mp4 => ExportFormat::Av1Mp4,
            Self::Av1Mkv => ExportFormat::Av1Mkv,
            Self::Av1WebM => ExportFormat::Av1WebM,
            Self::Av1Ivf => ExportFormat::Av1Ivf,
            Self::Opus => ExportFormat::Opus,
            Self::Flac => ExportFormat::Flac,
        }
    }

    /// All formats are shipped by the shared exporter, so the list here is the
    /// single source of truth for what the UI may offer.
    pub const ALL: [OutputFormat; 9] = [
        Self::Mp4H264,
        Self::H264Mkv,
        Self::MovH264,
        Self::Av1Mp4,
        Self::Av1Mkv,
        Self::Av1WebM,
        Self::Av1Ivf,
        Self::Opus,
        Self::Flac,
    ];
}

/// The container is decided by the format, so the file has to carry the
/// matching extension: AV1 bytes named `.mp4` lie to every player.
pub fn normalize_output_path(path: &std::path::Path, format: OutputFormat) -> PathBuf {
    let want = format.extension();
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(want))
    {
        return path.to_path_buf();
    }
    path.with_extension(want)
}

/// Export-time plan derived from the real render DAG (`miniter-render-plan`),
/// the same enumeration the exporter walks — not a parallel flat clip list.
/// `total_frames` is exact (per-frame rounded timestamps, no drift), and
/// `issues` carries graph validation findings so export fails deterministically
/// instead of encoding corrupt frames.
#[derive(Debug, Clone)]
pub struct ExportPlan {
    pub settings: RenderSettings,
    pub total_frames: u64,
    pub duration_us: i64,
    /// Max composited top-level layers seen across sampled frames.
    pub max_layers: usize,
    /// Validation findings, capped; empty when the graph is clean.
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityPreset {
    Draft,
    Preview,
    Standard,
    High,
    Master,
}

impl Default for QualityPreset {
    fn default() -> Self {
        Self::Standard
    }
}

impl QualityPreset {
    /// The video and audio bitrates (kbps) this preset aims for when the
    /// caller gave no explicit ones.
    pub fn default_bitrates(self) -> (u32, u32) {
        match self {
            Self::Draft => (2_500, 128),
            Self::Preview => (5_000, 160),
            Self::Standard => (8_000, 192),
            Self::High => (16_000, 256),
            Self::Master => (40_000, 320),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderSettings {
    pub output_path: PathBuf,
    pub format: OutputFormat,
    pub quality: QualityPreset,
    pub resolution: (u32, u32),
    pub fps: f64,
    pub video_bitrate: u32,
    pub audio_bitrate: u32,
    pub frame_range: Option<(i64, i64)>,
    pub use_hardware_accel: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            output_path: PathBuf::from("output.mp4"),
            format: OutputFormat::default(),
            quality: QualityPreset::default(),
            resolution: (1920, 1080),
            fps: 24.0,
            video_bitrate: 0,
            audio_bitrate: 0,
            frame_range: None,
            use_hardware_accel: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderResult {
    pub output_path: PathBuf,
    pub render_time_seconds: f64,
    pub file_size_bytes: u64,
}
