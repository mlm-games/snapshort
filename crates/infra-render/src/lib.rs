mod compositor;

use miniter_domain::{Clip, ClipId, ClipKind, Timeline, Timestamp, TrackId};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputFormat {
    Mp4H264,
    Mp4H265,
    WebmVp9,
    MovProRes,
    PngSequence,
    JpegSequence,
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self::Mp4H264
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderTransform {
    pub position: (f32, f32),
    pub scale: (f32, f32),
    pub rotation_deg: f32,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
}

impl Default for RenderTransform {
    fn default() -> Self {
        Self {
            position: (0.0, 0.0),
            scale: (1.0, 1.0),
            rotation_deg: 0.0,
            flip_horizontal: false,
            flip_vertical: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderColorAdjust {
    pub opacity: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
}

impl Default for RenderColorAdjust {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderEffects {
    pub transform: RenderTransform,
    pub color: RenderColorAdjust,
    pub speed: f32,
    pub reverse: bool,
    pub volume: f32,
}

impl Default for RenderEffects {
    fn default() -> Self {
        Self {
            transform: RenderTransform::default(),
            color: RenderColorAdjust::default(),
            speed: 1.0,
            reverse: false,
            volume: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderClip {
    pub clip_id: ClipId,
    pub source_path: String,
    pub clip_kind: ClipKind,
    pub track: miniter_domain::TrackId,
    pub timeline_start: Timestamp,
    pub timeline_end: Timestamp,
    pub source_start: miniter_domain::MediaDuration,
    pub source_end: miniter_domain::MediaDuration,
    pub effects: RenderEffects,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct RenderPlan {
    pub settings: RenderSettings,
    pub clips: Vec<RenderClip>,
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
pub struct RenderProgress {
    pub current_frame: i64,
    pub total_frames: i64,
    pub eta_seconds: Option<f64>,
    pub phase: RenderPhase,
}

impl RenderProgress {
    pub fn percentage(&self) -> f64 {
        if self.total_frames == 0 {
            0.0
        } else {
            self.current_frame as f64 / self.total_frames as f64
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderPhase {
    Preparing,
    RenderingVideo,
    EncodingAudio,
    Muxing,
    Finalizing,
    Complete,
}

#[derive(Debug, Clone)]
pub struct RenderResult {
    pub output_path: PathBuf,
    pub render_time_seconds: f64,
    pub file_size_bytes: u64,
}

#[derive(Debug, Clone)]
pub enum RenderError {
    InvalidSettings(String),
    CodecNotAvailable(String),
    IoError(String),
    EncodingError(String),
    Cancelled,
    HardwareAccelFailed(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSettings(msg) => write!(f, "Invalid render settings: {}", msg),
            Self::CodecNotAvailable(codec) => write!(f, "Codec not available: {}", codec),
            Self::IoError(msg) => write!(f, "IO error: {}", msg),
            Self::EncodingError(msg) => write!(f, "Encoding error: {}", msg),
            Self::Cancelled => write!(f, "Render cancelled"),
            Self::HardwareAccelFailed(msg) => write!(f, "Hardware acceleration failed: {}", msg),
        }
    }
}

impl std::error::Error for RenderError {}

pub struct RenderService {
    hardware_accel_available: bool,
}

impl Default for RenderService {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderService {
    pub fn new() -> Self {
        Self {
            hardware_accel_available: false,
        }
    }

    pub fn is_format_supported(&self, format: &OutputFormat) -> bool {
        matches!(format, OutputFormat::Mp4H264)
    }

    pub fn is_hardware_accel_available(&self) -> bool {
        self.hardware_accel_available
    }

    pub fn validate_settings(&self, settings: &RenderSettings) -> Result<(), RenderError> {
        if settings.resolution.0 == 0 || settings.resolution.1 == 0 {
            return Err(RenderError::InvalidSettings("Resolution must be non-zero".into()));
        }
        if settings.fps <= 0.0 {
            return Err(RenderError::InvalidSettings("Frame rate must be positive".into()));
        }
        if !self.is_format_supported(&settings.format) {
            return Err(RenderError::CodecNotAvailable(format!("{:?}", settings.format)));
        }
        Ok(())
    }

    pub fn start_render(
        &self,
        _timeline: &Timeline,
        settings: RenderSettings,
    ) -> Result<RenderJobHandle, RenderError> {
        self.validate_settings(&settings)?;
        Ok(RenderJobHandle {
            id: uuid::Uuid::new_v4(),
            settings,
            cancelled: false,
        })
    }

    pub fn render_preview_frame(
        &self,
        timeline: &Timeline,
        frame: Timestamp,
    ) -> Result<Vec<u8>, RenderError> {
        let (w, h) = (1920, 1080);
        compositor::render_preview_frame(timeline, frame, w, h)
    }

    pub fn render_thumbnail(
        &self,
        source_path: &str,
        time_us: i64,
    ) -> Result<Vec<u8>, RenderError> {
        compositor::render_thumbnail(source_path, time_us)
    }

    pub fn export_timeline(
        &self,
        timeline: &Timeline,
        settings: &RenderSettings,
        track_volumes: &HashMap<TrackId, f32>,
        master_volume: f32,
    ) -> Result<RenderResult, RenderError> {
        self.validate_settings(settings)?;

        use miniter_domain::export::{ExportFormat, ExportProfile, ExportResolution, SubtitleMode};
        use miniter_domain::project::{Project, ProjectId, ProjectMeta};
        use std::time::SystemTime;

        let export_format = match settings.format {
            OutputFormat::Mp4H264 => ExportFormat::Mp4,
            _ => return Err(RenderError::CodecNotAvailable(
                format!("{:?} not supported by miniter exporter", settings.format),
            )),
        };

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let mut export_timeline = timeline.clone();
        for track in &mut export_timeline.tracks {
            let track_vol = track_volumes.get(&track.id).copied().unwrap_or(1.0);
            for clip in &mut track.clips {
                clip.volume = (clip.volume * track_vol * master_volume).clamp(0.0, 2.0);
            }
        }

        let project = Project {
            id: ProjectId::new(),
            meta: ProjectMeta {
                name: "snapshort-export".into(),
                created_at: now,
                modified_at: now,
                schema_version: 2,
            },
            timeline: export_timeline,
            export_profile: ExportProfile {
                format: export_format,
                resolution: ExportResolution::Custom {
                    width: settings.resolution.0,
                    height: settings.resolution.1,
                },
                fps: settings.fps,
                video_bitrate_kbps: settings.video_bitrate.max(500),
                audio_bitrate_kbps: settings.audio_bitrate.max(128),
                audio_sample_rate: 48000,
                output_path: settings.output_path.to_string_lossy().into(),
                subtitle_mode: SubtitleMode::Hard,
                hardware_acceleration: settings.use_hardware_accel,
            },
        };

        let start = std::time::Instant::now();
        miniter_media_native::export::export_project(
            &project,
            &settings.output_path,
            || false,
            |_| {},
        )
        .map_err(|e| RenderError::EncodingError(e.to_string()))?;

        let render_time = start.elapsed().as_secs_f64();
        let file_size = std::fs::metadata(&settings.output_path)
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(RenderResult {
            output_path: settings.output_path.clone(),
            render_time_seconds: render_time,
            file_size_bytes: file_size,
        })
    }

    pub fn recommended_settings(&self, timeline: &Timeline) -> RenderSettings {
        RenderSettings {
            output_path: PathBuf::from("timeline_export.mp4"),
            format: OutputFormat::Mp4H264,
            quality: QualityPreset::Standard,
            resolution: (1920, 1080),
            fps: 30.0,
            video_bitrate: 0,
            audio_bitrate: 0,
            frame_range: None,
            use_hardware_accel: self.hardware_accel_available,
        }
    }

    pub fn build_render_plan(&self, timeline: &Timeline, settings: RenderSettings) -> RenderPlan {
        let mut clips: Vec<RenderClip> = Vec::new();

        for track in &timeline.tracks {
            if track.locked {
                continue;
            }
            for clip in &track.clips {
                if clip.muted {
                    continue;
                }

                let effects = render_effects_from_clip(clip);

                let (source_path, clip_kind) = match &clip.kind {
                    ClipKind::Video(v) => (v.source_path.clone(), clip.kind.clone()),
                    ClipKind::Audio(a) => (a.source_path.clone(), clip.kind.clone()),
                    ClipKind::Text(t) => (String::new(), clip.kind.clone()),
                    ClipKind::Subtitle(s) => (s.source_path.clone(), clip.kind.clone()),
                    _ => (String::new(), clip.kind.clone()),
                };

                clips.push(RenderClip {
                    clip_id: clip.id,
                    source_path,
                    clip_kind,
                    track: track.id,
                    timeline_start: clip.timeline_start,
                    timeline_end: clip.timeline_end(),
                    source_start: clip.source_start,
                    source_end: clip.source_end,
                    effects,
                    enabled: true,
                });
            }
        }

        RenderPlan { settings, clips }
    }
}

fn render_effects_from_clip(clip: &Clip) -> RenderEffects {
    RenderEffects {
        transform: RenderTransform {
            position: (0.0, 0.0),
            scale: (1.0, 1.0),
            rotation_deg: 0.0,
            flip_horizontal: false,
            flip_vertical: false,
        },
        color: RenderColorAdjust {
            opacity: clip.opacity.clamp(0.0, 1.0),
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
        },
        speed: clip.speed.clamp(0.1, 10.0) as f32,
        reverse: false,
        volume: clip.volume.clamp(0.0, 2.0),
    }
}

pub struct RenderJobHandle {
    pub id: uuid::Uuid,
    pub settings: RenderSettings,
    cancelled: bool,
}

impl RenderJobHandle {
    pub fn progress(&self) -> RenderProgress {
        RenderProgress {
            current_frame: 0,
            total_frames: 0,
            eta_seconds: None,
            phase: RenderPhase::Complete,
        }
    }

    pub fn is_complete(&self) -> bool { true }
    pub fn is_cancelled(&self) -> bool { self.cancelled }

    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    pub fn wait(self) -> Result<RenderResult, RenderError> {
        if self.cancelled {
            return Err(RenderError::Cancelled);
        }
        if let Some(parent) = self.settings.output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| RenderError::IoError(e.to_string()))?;
        }
        std::fs::write(&self.settings.output_path, b"snapshort export placeholder")
            .map_err(|e| RenderError::IoError(e.to_string()))?;
        Ok(RenderResult {
            output_path: self.settings.output_path,
            render_time_seconds: 0.0,
            file_size_bytes: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_settings_default() {
        let settings = RenderSettings::default();
        assert_eq!(settings.resolution, (1920, 1080));
        assert_eq!(settings.fps, 24.0);
    }

    #[test]
    fn test_render_service_validate() {
        let service = RenderService::new();
        let mut settings = RenderSettings::default();
        assert!(service.validate_settings(&settings).is_ok());
        settings.resolution = (0, 0);
        assert!(service.validate_settings(&settings).is_err());
    }

    #[test]
    fn test_render_progress_percentage() {
        let progress = RenderProgress {
            current_frame: 50,
            total_frames: 100,
            eta_seconds: None,
            phase: RenderPhase::RenderingVideo,
        };
        assert_eq!(progress.percentage(), 0.5);
    }
}
