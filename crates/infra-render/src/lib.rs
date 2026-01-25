//! Video rendering infrastructure
//!
//! This crate provides video rendering capabilities for the Snapshort video editor.
//! It handles encoding, frame composition, and export functionality.

use snapshort_domain::prelude::*;
use std::path::PathBuf;

/// Render output format
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputFormat {
    /// H.264/AVC codec in MP4 container
    Mp4H264,
    /// H.265/HEVC codec in MP4 container
    Mp4H265,
    /// VP9 codec in WebM container
    WebmVp9,
    /// ProRes codec in MOV container (for professional workflows)
    MovProRes,
    /// PNG image sequence
    PngSequence,
    /// JPEG image sequence
    JpegSequence,
}

/// Render transform parameters derived from clip effects.
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

/// Render color parameters derived from clip effects.
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

/// Render effects for a clip. This is the stable, renderer-facing form.
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

impl From<&ClipEffects> for RenderEffects {
    fn from(effects: &ClipEffects) -> Self {
        RenderEffects {
            transform: RenderTransform {
                position: effects.position,
                scale: (
                    effects.scale.0.clamp(0.1, 10.0),
                    effects.scale.1.clamp(0.1, 10.0),
                ),
                rotation_deg: effects.rotation.clamp(-180.0, 180.0),
                flip_horizontal: effects.flip_horizontal,
                flip_vertical: effects.flip_vertical,
            },
            color: RenderColorAdjust {
                opacity: effects.opacity.clamp(0.0, 1.0),
                brightness: effects.brightness.clamp(-1.0, 1.0),
                contrast: effects.contrast.clamp(-1.0, 1.0),
                saturation: effects.saturation.clamp(-1.0, 1.0),
            },
            speed: effects.speed.clamp(0.1, 10.0),
            reverse: effects.reverse,
            volume: effects.volume.clamp(0.0, 2.0),
        }
    }
}

/// A render-ready clip description derived from the timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderClip {
    pub clip_id: ClipId,
    pub clip_type: ClipType,
    pub asset_id: Option<AssetId>,
    pub track: TrackRef,
    pub timeline_start: Frame,
    pub timeline_end: Frame,
    pub source_range: FrameRange,
    pub effects: RenderEffects,
    pub enabled: bool,
    pub locked: bool,
}

/// Render plan produced from a timeline and render settings.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderPlan {
    pub timeline_id: TimelineId,
    pub settings: RenderSettings,
    pub clips: Vec<RenderClip>,
}

/// Resolved source for export.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportSource {
    pub path: PathBuf,
    pub source_range: FrameRange,
    pub source_fps: Fps,
    pub effects: RenderEffects,
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self::Mp4H264
    }
}

/// Video quality preset
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityPreset {
    /// Fast encoding, lower quality
    Draft,
    /// Balanced encoding speed and quality
    Preview,
    /// Standard quality for most uses
    Standard,
    /// High quality, slower encoding
    High,
    /// Maximum quality, slowest encoding
    Master,
}

impl Default for QualityPreset {
    fn default() -> Self {
        Self::Standard
    }
}

/// Render settings for export
#[derive(Debug, Clone, PartialEq)]
pub struct RenderSettings {
    /// Output file path
    pub output_path: PathBuf,
    /// Output format/codec
    pub format: OutputFormat,
    /// Quality preset
    pub quality: QualityPreset,
    /// Output resolution (width, height)
    pub resolution: (u32, u32),
    /// Frame rate
    pub fps: f64,
    /// Video bitrate in kbps (0 = auto)
    pub video_bitrate: u32,
    /// Audio bitrate in kbps (0 = auto)
    pub audio_bitrate: u32,
    /// Range of frames to render (None = entire timeline)
    pub frame_range: Option<FrameRange>,
    /// Enable hardware acceleration if available
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

/// Render progress information
#[derive(Debug, Clone)]
pub struct RenderProgress {
    /// Current frame being rendered
    pub current_frame: i64,
    /// Total frames to render
    pub total_frames: i64,
    /// Estimated time remaining in seconds
    pub eta_seconds: Option<f64>,
    /// Current render phase
    pub phase: RenderPhase,
}

impl RenderProgress {
    /// Calculate completion percentage (0.0 - 1.0)
    pub fn percentage(&self) -> f64 {
        if self.total_frames == 0 {
            0.0
        } else {
            self.current_frame as f64 / self.total_frames as f64
        }
    }
}

/// Current phase of the render process
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderPhase {
    /// Preparing render job
    Preparing,
    /// Rendering video frames
    RenderingVideo,
    /// Encoding audio
    EncodingAudio,
    /// Muxing audio and video
    Muxing,
    /// Finalizing output file
    Finalizing,
    /// Render complete
    Complete,
}

/// Result of a render operation
#[derive(Debug, Clone)]
pub struct RenderResult {
    /// Path to the rendered file
    pub output_path: PathBuf,
    /// Total render time in seconds
    pub render_time_seconds: f64,
    /// Output file size in bytes
    pub file_size_bytes: u64,
}

/// Render error types
#[derive(Debug, Clone)]
pub enum RenderError {
    /// Invalid render settings
    InvalidSettings(String),
    /// Codec not available
    CodecNotAvailable(String),
    /// IO error during render
    IoError(String),
    /// Encoding error
    EncodingError(String),
    /// Render was cancelled
    Cancelled,
    /// Hardware acceleration failed
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

/// Render service for video export
///
/// This is a stub implementation. In a real implementation, this would
/// interface with FFmpeg or a similar video encoding library.
pub struct RenderService {
    /// Whether hardware acceleration is available
    hardware_accel_available: bool,
}

impl Default for RenderService {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderService {
    /// Create a new render service
    pub fn new() -> Self {
        Self {
            // In a real implementation, this would probe for hardware encoders
            hardware_accel_available: false,
        }
    }

    /// Check if a specific output format is supported
    pub fn is_format_supported(&self, format: &OutputFormat) -> bool {
        // In a real implementation, this would check for codec availability
        matches!(
            format,
            OutputFormat::Mp4H264
                | OutputFormat::Mp4H265
                | OutputFormat::PngSequence
                | OutputFormat::JpegSequence
        )
    }

    /// Check if hardware acceleration is available
    pub fn is_hardware_accel_available(&self) -> bool {
        self.hardware_accel_available
    }

    /// Validate render settings
    pub fn validate_settings(&self, settings: &RenderSettings) -> Result<(), RenderError> {
        // Check resolution
        if settings.resolution.0 == 0 || settings.resolution.1 == 0 {
            return Err(RenderError::InvalidSettings(
                "Resolution must be non-zero".into(),
            ));
        }

        // Check fps
        if settings.fps <= 0.0 {
            return Err(RenderError::InvalidSettings(
                "Frame rate must be positive".into(),
            ));
        }

        // Check format support
        if !self.is_format_supported(&settings.format) {
            return Err(RenderError::CodecNotAvailable(format!(
                "{:?}",
                settings.format
            )));
        }

        Ok(())
    }

    /// Start a render job (stub implementation)
    ///
    /// In a real implementation, this would spawn a background task
    /// and return a handle for progress monitoring.
    pub fn start_render(
        &self,
        _timeline: &Timeline,
        settings: RenderSettings,
    ) -> Result<RenderJobHandle, RenderError> {
        self.validate_settings(&settings)?;

        // Stub: Return a job handle that immediately completes
        Ok(RenderJobHandle {
            id: uuid::Uuid::new_v4(),
            settings,
            cancelled: false,
        })
    }

    /// Export a single clip using ffmpeg (MVP path).
    pub fn export_single_clip(
        &self,
        source: ExportSource,
        settings: RenderSettings,
    ) -> Result<RenderResult, RenderError> {
        self.validate_settings(&settings)?;

        if !matches!(settings.format, OutputFormat::Mp4H264) {
            return Err(RenderError::CodecNotAvailable(
                "Only MP4 H.264 is supported in MVP export".into(),
            ));
        }

        let ffmpeg_ok = std::process::Command::new("ffmpeg")
            .arg("-version")
            .output();
        if ffmpeg_ok.is_err() {
            return Err(RenderError::CodecNotAvailable(
                "ffmpeg not found in PATH".into(),
            ));
        }

        let start_sec = source.source_range.start.0 as f64 / source.source_fps.as_f64();
        let duration_sec = source.source_range.duration() as f64 / source.source_fps.as_f64();

        if let Some(parent) = settings.output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| RenderError::IoError(e.to_string()))?;
        }

        let scale_filter = build_video_filter(settings.resolution, &source.effects);

        let output = std::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-ss")
            .arg(format!("{start_sec:.3}"))
            .arg("-i")
            .arg(&source.path)
            .arg("-t")
            .arg(format!("{duration_sec:.3}"))
            .arg("-vf")
            .arg(scale_filter)
            .arg("-r")
            .arg(format!("{:.3}", settings.fps))
            .arg("-c:v")
            .arg("libx264")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg(&settings.output_path)
            .output();

        let output = output.map_err(|e| RenderError::IoError(e.to_string()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(RenderError::EncodingError(stderr));
        }

        let file_size = std::fs::metadata(&settings.output_path)
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(RenderResult {
            output_path: settings.output_path,
            render_time_seconds: 0.0,
            file_size_bytes: file_size,
        })
    }

    /// Export a timeline by concatenating the enabled video clips.
    pub fn export_timeline(
        &self,
        sources: &[ExportSource],
        settings: RenderSettings,
    ) -> Result<RenderResult, RenderError> {
        self.validate_settings(&settings)?;

        if sources.is_empty() {
            return Err(RenderError::InvalidSettings(
                "No export sources provided".into(),
            ));
        }

        if !matches!(settings.format, OutputFormat::Mp4H264) {
            return Err(RenderError::CodecNotAvailable(
                "Only MP4 H.264 is supported in MVP export".into(),
            ));
        }

        let ffmpeg_ok = std::process::Command::new("ffmpeg")
            .arg("-version")
            .output();
        if ffmpeg_ok.is_err() {
            return Err(RenderError::CodecNotAvailable(
                "ffmpeg not found in PATH".into(),
            ));
        }

        if let Some(parent) = settings.output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| RenderError::IoError(e.to_string()))?;
        }

        let mut filter = String::new();
        let mut concat_refs: Vec<String> = Vec::new();

        for (idx, source) in sources.iter().enumerate() {
            let start_sec = source.source_range.start.0 as f64 / source.source_fps.as_f64();
            let duration_sec = source.source_range.duration() as f64 / source.source_fps.as_f64();
            let vf = build_video_filter(settings.resolution, &source.effects);

            filter.push_str(&format!(
                "[{idx}:v]trim=start={start:.3}:duration={dur:.3},setpts=PTS-STARTPTS,{vf}[v{idx}];",
                idx = idx,
                start = start_sec,
                dur = duration_sec,
                vf = vf
            ));
            concat_refs.push(format!("[v{idx}]"));
        }

        filter.push_str(&format!(
            "{}concat=n={}:v=1:a=0[vout]",
            concat_refs.join(""),
            sources.len()
        ));

        let mut cmd = std::process::Command::new("ffmpeg");
        cmd.arg("-y");

        for source in sources {
            cmd.arg("-i").arg(&source.path);
        }

        let output = cmd
            .arg("-filter_complex")
            .arg(filter)
            .arg("-map")
            .arg("[vout]")
            .arg("-r")
            .arg(format!("{:.3}", settings.fps))
            .arg("-c:v")
            .arg("libx264")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg(&settings.output_path)
            .output();

        let output = output.map_err(|e| RenderError::IoError(e.to_string()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(RenderError::EncodingError(stderr));
        }

        let file_size = std::fs::metadata(&settings.output_path)
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(RenderResult {
            output_path: settings.output_path,
            render_time_seconds: 0.0,
            file_size_bytes: file_size,
        })
    }

    /// Get recommended settings for a timeline
    pub fn recommended_settings(&self, timeline: &Timeline) -> RenderSettings {
        let duration = timeline.duration();
        RenderSettings {
            output_path: PathBuf::from(format!("{}_export.mp4", timeline.name)),
            format: OutputFormat::Mp4H264,
            quality: QualityPreset::Standard,
            resolution: (
                timeline.settings.resolution.width,
                timeline.settings.resolution.height,
            ),
            fps: timeline.settings.fps.as_f64(),
            video_bitrate: 0,
            audio_bitrate: 0,
            frame_range: Some(FrameRange::new_unchecked(0, duration.0)),
            use_hardware_accel: self.hardware_accel_available,
        }
    }

    /// Build a render plan from a timeline and settings.
    pub fn build_render_plan(&self, timeline: &Timeline, settings: RenderSettings) -> RenderPlan {
        let mut clips: Vec<RenderClip> = timeline
            .clips
            .iter()
            .filter(|clip| clip.enabled)
            .map(|clip| RenderClip {
                clip_id: clip.id,
                clip_type: clip.clip_type.clone(),
                asset_id: clip.asset_id,
                track: clip.track,
                timeline_start: clip.timeline_start,
                timeline_end: clip.timeline_end(),
                source_range: clip.source_range,
                effects: RenderEffects::from(&clip.effects),
                enabled: clip.enabled,
                locked: clip.locked,
            })
            .collect();

        clips.sort_by_key(|clip| {
            let track_order = match clip.track.track_type {
                TrackType::Video => 0u8,
                TrackType::Audio => 1u8,
            };
            (track_order, clip.track.index, clip.timeline_start.0)
        });

        RenderPlan {
            timeline_id: timeline.id,
            settings,
            clips,
        }
    }
}

pub fn build_video_filter(resolution: (u32, u32), effects: &RenderEffects) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!(
        "scale={}x{}:flags=lanczos",
        resolution.0, resolution.1
    ));

    let b = effects.color.brightness;
    let c = (1.0 + effects.color.contrast).clamp(0.0, 2.0);
    let s = (1.0 + effects.color.saturation).clamp(0.0, 2.0);
    if b.abs() > f32::EPSILON || (c - 1.0).abs() > f32::EPSILON || (s - 1.0).abs() > f32::EPSILON {
        parts.push(format!(
            "eq=brightness={:.3}:contrast={:.3}:saturation={:.3}",
            b, c, s
        ));
    }

    parts.join(",")
}

/// Handle to a running render job
pub struct RenderJobHandle {
    /// Unique job ID
    pub id: uuid::Uuid,
    /// Render settings
    pub settings: RenderSettings,
    /// Whether the job was cancelled
    cancelled: bool,
}

impl RenderJobHandle {
    /// Get current render progress (stub)
    pub fn progress(&self) -> RenderProgress {
        // Stub: Return completed progress
        RenderProgress {
            current_frame: 0,
            total_frames: 0,
            eta_seconds: None,
            phase: RenderPhase::Complete,
        }
    }

    /// Check if render is complete
    pub fn is_complete(&self) -> bool {
        // Stub: Always complete
        true
    }

    /// Check if render was cancelled
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Cancel the render job
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// Wait for render to complete and get result (stub)
    pub fn wait(self) -> Result<RenderResult, RenderError> {
        if self.cancelled {
            return Err(RenderError::Cancelled);
        }

        if let Some(parent) = self.settings.output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| RenderError::IoError(e.to_string()))?;
        }
        std::fs::write(&self.settings.output_path, b"snapshort export placeholder")
            .map_err(|e| RenderError::IoError(e.to_string()))?;

        // Stub: Return fake result
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

    #[test]
    fn test_build_render_plan_maps_effects() {
        let timeline = Timeline::new("Test");
        let mut clip = Clip::from_asset(
            AssetId::new(),
            ClipType::Video,
            FrameRange::new_unchecked(0, 120),
            Frame(10),
            TrackRef::video(0),
        );
        clip.effects.opacity = 0.5;
        clip.effects.position = (120.0, -40.0);
        clip.effects.scale = (1.5, 0.8);
        clip.effects.rotation = 30.0;
        clip.effects.brightness = 0.2;
        clip.effects.contrast = -0.1;
        clip.effects.saturation = 0.4;

        let timeline = timeline.insert_clip(clip).unwrap();
        let service = RenderService::new();
        let settings = service.recommended_settings(&timeline);

        let plan = service.build_render_plan(&timeline, settings);
        assert_eq!(plan.clips.len(), 1);

        let render_clip = &plan.clips[0];
        assert_eq!(render_clip.timeline_start.0, 10);
        assert_eq!(render_clip.timeline_end.0, 130);
        assert_eq!(render_clip.effects.color.opacity, 0.5);
        assert_eq!(render_clip.effects.transform.position, (120.0, -40.0));
        assert_eq!(render_clip.effects.transform.scale, (1.5, 0.8));
        assert_eq!(render_clip.effects.transform.rotation_deg, 30.0);
        assert_eq!(render_clip.effects.color.brightness, 0.2);
        assert_eq!(render_clip.effects.color.contrast, -0.1);
        assert_eq!(render_clip.effects.color.saturation, 0.4);
    }
}
