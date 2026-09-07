#[cfg(not(target_arch = "wasm32"))]
mod compositor;

use miniter_domain::{Timeline, Timestamp, TrackId};
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

#[cfg(not(target_arch = "wasm32"))]
pub struct RenderService {
    hardware_accel_available: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for RenderService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
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

        // Deterministic failure: refuse to encode a timeline whose render
        // graph is corrupt instead of producing garbage output.
        let plan = self.build_render_plan(timeline, settings.clone());
        if !plan.issues.is_empty() {
            return Err(RenderError::InvalidSettings(format!(
                "render graph invalid ({} issue(s), first: {})",
                plan.issues.len(),
                plan.issues[0]
            )));
        }

        use miniter_domain::export::{ExportFormat, ExportProfile, ExportResolution, SubtitleMode};
        use miniter_domain::project::{Project, ProjectId, ProjectMeta};
        use web_time::SystemTime;

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

        let start = web_time::Instant::now();
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

    /// Build the export plan from the real render DAG: exact frame count from
    /// the deterministic iterator plus validation findings over sampled
    /// frames (bounded work even for hour-long timelines; first and last
    /// frames always sampled).
    pub fn build_render_plan(&self, timeline: &Timeline, settings: RenderSettings) -> ExportPlan {
        use miniter_domain::export::SubtitleMode;
        use miniter_render_plan::compositor::FramePlanIterator;
        use miniter_render_plan::{validate_frame_plan, RenderNode};

        let duration_us = timeline.duration_end().as_micros().max(0);
        let iter = FramePlanIterator::with_render_settings(
            timeline,
            settings.resolution.0,
            settings.resolution.1,
            settings.fps,
            SubtitleMode::Hard,
        );
        let total_frames = iter.total_frames();

        fn layers_of(root: &RenderNode) -> usize {
            match root {
                RenderNode::Stack(nodes) => nodes.len(),
                _ => 1,
            }
        }

        let mut max_layers = 0usize;
        let mut issues = Vec::new();
        if total_frames > 0 {
            // Sample ~2000 frames max; always include the last one.
            let stride = (total_frames / 2000).max(1);
            for (i, plan) in iter.enumerate() {
                let is_last = i as u64 + 1 == total_frames;
                if i as u64 % stride != 0 && !is_last {
                    continue;
                }
                max_layers = max_layers.max(layers_of(&plan.root));
                if issues.len() < MAX_PLAN_ISSUES {
                    for violation in validate_frame_plan(&plan) {
                        if issues.len() >= MAX_PLAN_ISSUES {
                            break;
                        }
                        issues.push(format!(
                            "frame {} ({}µs): {violation:?}",
                            i,
                            plan.timestamp.as_micros()
                        ));
                    }
                }
            }
        }

        ExportPlan {
            settings,
            total_frames,
            duration_us,
            max_layers,
            issues,
        }
    }
}

/// Cap for reported plan validation findings (sampling itself is bounded).
const MAX_PLAN_ISSUES: usize = 8;

#[cfg(not(target_arch = "wasm32"))]
pub struct RenderJobHandle {
    pub id: uuid::Uuid,
    pub settings: RenderSettings,
    cancelled: bool,
}

#[cfg(not(target_arch = "wasm32"))]
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
    use miniter_domain::clip::{Clip, ClipId, ClipKind, VideoClip};
    use miniter_domain::time::{MediaDuration, Timestamp};
    use miniter_domain::track::{Track, TrackKind};

    fn video_clip(start_us: i64, dur_us: i64) -> Clip {
        Clip {
            id: ClipId(uuid::Uuid::new_v4()),
            timeline_start: Timestamp::from_micros(start_us),
            timeline_duration: MediaDuration::from_micros(dur_us),
            source_start: MediaDuration::ZERO,
            source_end: MediaDuration::from_micros(dur_us),
            source_total_duration: MediaDuration::from_micros(dur_us),
            speed: 1.0,
            volume: 1.0,
            opacity: 1.0,
            muted: false,
            transition_in: None,
            transition_out: None,
            kind: ClipKind::Video(VideoClip {
                source_path: "/tmp/does-not-need-to-exist.mp4".into(),
                width: 1920,
                height: 1080,
                fps: 30.0,
                filters: vec![],
                audio_filters: vec![],
                masks: vec![],
            }),
            keyframes: Default::default(),
            blend_mode: Default::default(),
        }
    }

    fn timeline_with(clips: Vec<Clip>) -> Timeline {
        let mut track = Track::new(TrackKind::Video, "V1");
        for clip in clips {
            track.insert_clip(clip).expect("fixture clip inserts");
        }
        Timeline {
            tracks: vec![track],
        }
    }

    fn test_settings() -> RenderSettings {
        RenderSettings {
            output_path: std::path::PathBuf::from("/tmp/snapshort-plan-test.mp4"),
            format: OutputFormat::Mp4H264,
            quality: QualityPreset::Standard,
            resolution: (1920, 1080),
            fps: 30.0,
            video_bitrate: 8000,
            audio_bitrate: 192,
            frame_range: None,
            use_hardware_accel: false,
        }
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
    fn export_plan_counts_frames_layers_and_empties() {
        // Pure planning: no media is opened, so missing files are fine.
        let service = RenderService::new();
        let timeline = timeline_with(vec![video_clip(0, 10_000_000)]);
        let plan = service.build_render_plan(&timeline, test_settings());
        assert_eq!(plan.total_frames, 300);
        assert_eq!(plan.duration_us, 10_000_000);
        assert_eq!(plan.max_layers, 1);
        assert!(plan.issues.is_empty());

        let empty = service.build_render_plan(&Timeline::new(), test_settings());
        assert_eq!(empty.total_frames, 0);
        assert!(empty.issues.is_empty());
    }

    #[test]
    fn corrupt_graph_flagged_and_export_refused() {
        let service = RenderService::new();
        let mut bad = video_clip(0, 10_000_000);
        bad.speed = -2.0; // bypasses normalize; planning must catch it
        let timeline = timeline_with(vec![bad]);
        let plan = service.build_render_plan(&timeline, test_settings());
        assert!(!plan.issues.is_empty());

        // …and export fails in validation, before any encoder/media work.
        let err = service
            .export_timeline(&timeline, &test_settings(), &HashMap::new(), 1.0)
            .unwrap_err();
        assert!(
            matches!(err, RenderError::InvalidSettings(_)),
            "expected InvalidSettings, got {err:?}"
        );
    }
}
