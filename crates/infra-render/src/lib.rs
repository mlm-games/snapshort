mod compositor;
mod timeline_ffmpeg;

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

#[derive(Debug, Clone, PartialEq)]
pub struct ExportSource {
    pub path: PathBuf,
    pub source_start_us: i64,
    pub source_end_us: i64,
    pub source_fps: f64,
    pub effects: RenderEffects,
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

    pub fn export_single_clip(
        &self,
        source: ExportSource,
        settings: RenderSettings,
    ) -> Result<RenderResult, RenderError> {
        self.validate_settings(&settings)?;
        if !matches!(settings.format, OutputFormat::Mp4H264) {
            return Err(RenderError::CodecNotAvailable("Only MP4 H.264 is supported in MVP export".into()));
        }

        let ffmpeg_ok = std::process::Command::new("ffmpeg").arg("-version").output();
        if ffmpeg_ok.is_err() {
            return Err(RenderError::CodecNotAvailable("ffmpeg not found in PATH".into()));
        }

        let duration_sec = (source.source_end_us - source.source_start_us) as f64 / 1_000_000.0;
        let start_sec = source.source_start_us as f64 / 1_000_000.0;

        if let Some(parent) = settings.output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| RenderError::IoError(e.to_string()))?;
        }

        let scale_filter = build_video_filter(settings.resolution, &source.effects);

        let output = std::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-ss").arg(format!("{start_sec:.3}"))
            .arg("-i").arg(&source.path)
            .arg("-t").arg(format!("{duration_sec:.3}"))
            .arg("-vf").arg(scale_filter)
            .arg("-r").arg(format!("{:.3}", settings.fps))
            .arg("-map").arg("0:v:0")
            .arg("-map").arg("0:a?")
            .arg("-c:v").arg("libx264")
            .arg("-c:a").arg("aac")
            .arg("-b:a").arg("128k")
            .arg("-pix_fmt").arg("yuv420p")
            .arg(&settings.output_path)
            .output();

        let output = output.map_err(|e| RenderError::IoError(e.to_string()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(RenderError::EncodingError(stderr));
        }

        let file_size = std::fs::metadata(&settings.output_path)
            .map(|m| m.len()).unwrap_or(0);

        Ok(RenderResult {
            output_path: settings.output_path,
            render_time_seconds: 0.0,
            file_size_bytes: file_size,
        })
    }

    pub fn export_timeline(
        &self,
        timeline: &Timeline,
        settings: RenderSettings,
        track_volumes: &HashMap<TrackId, f32>,
        master_volume: f32,
    ) -> Result<RenderResult, RenderError> {
        self.validate_settings(&settings)?;
        timeline_ffmpeg::export_timeline(timeline, settings, track_volumes, master_volume)
    }

    pub fn render_preview_frame(
        &self,
        timeline: &Timeline,
        frame: Timestamp,
    ) -> Result<Vec<u8>, RenderError> {
        let (w, h) = (1920, 1080);
        compositor::render_preview_frame(timeline, frame, w, h)
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

pub(crate) fn video_filter_effects(effects: &RenderEffects, resolution: (u32, u32)) -> Vec<String> {
    let mut parts = Vec::new();

    parts.push(format!(
        "scale={}:{}:force_original_aspect_ratio=decrease:flags=lanczos",
        resolution.0, resolution.1
    ));

    if (effects.transform.scale.0 - 1.0).abs() > f32::EPSILON
        || (effects.transform.scale.1 - 1.0).abs() > f32::EPSILON
    {
        parts.push(format!(
            "scale='max(2,trunc(iw*{:.6}/2)*2)':'max(2,trunc(ih*{:.6}/2)*2)'",
            effects.transform.scale.0.max(0.1),
            effects.transform.scale.1.max(0.1)
        ));
    }

    if effects.transform.flip_horizontal {
        parts.push("hflip".into());
    }
    if effects.transform.flip_vertical {
        parts.push("vflip".into());
    }
    if effects.transform.rotation_deg.abs() > f32::EPSILON {
        parts.push(format!(
            "rotate={:.6}*PI/180:c=none:ow=rotw(iw):oh=roth(ih)",
            effects.transform.rotation_deg
        ));
    }

    let contrast = (1.0 + effects.color.contrast).clamp(0.0, 2.0);
    let saturation = (1.0 + effects.color.saturation).clamp(0.0, 2.0);
    if effects.color.brightness.abs() > f32::EPSILON
        || (contrast - 1.0).abs() > f32::EPSILON
        || (saturation - 1.0).abs() > f32::EPSILON
    {
        parts.push(format!(
            "eq=brightness={:.6}:contrast={:.6}:saturation={:.6}",
            effects.color.brightness, contrast, saturation
        ));
    }

    if (effects.color.opacity - 1.0).abs() > f32::EPSILON {
        parts.push(format!(
            "colorchannelmixer=aa={:.6}",
            effects.color.opacity.clamp(0.0, 1.0)
        ));
    }

    parts
}

pub fn build_video_filter(resolution: (u32, u32), effects: &RenderEffects) -> String {
    video_filter_effects(effects, resolution).join(",")
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
