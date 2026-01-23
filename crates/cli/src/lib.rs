//! CLI library for Snapshort Video Editor
//!
//! Provides command-line interface functionality including project management,
//! media analysis, and batch processing operations.

use std::path::PathBuf;

/// CLI configuration
#[derive(Debug, Clone)]
pub struct CliConfig {
    /// Whether to enable verbose output
    pub verbose: bool,
    /// Output format for structured data
    pub output_format: OutputFormat,
    /// Working directory
    pub working_dir: PathBuf,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            verbose: false,
            output_format: OutputFormat::Text,
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

/// Output format for CLI commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable text
    Text,
    /// JSON format
    Json,
    /// YAML format
    Yaml,
}

/// Result of a CLI operation
#[derive(Debug)]
pub struct CliResult {
    /// Whether the operation succeeded
    pub success: bool,
    /// Output message
    pub message: String,
    /// Exit code
    pub exit_code: i32,
}

impl CliResult {
    /// Create a success result
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            exit_code: 0,
        }
    }

    /// Create a failure result
    pub fn failure(message: impl Into<String>, exit_code: i32) -> Self {
        Self {
            success: false,
            message: message.into(),
            exit_code,
        }
    }
}

/// Project information for CLI output
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    /// Project name
    pub name: String,
    /// Project file path
    pub path: Option<PathBuf>,
    /// Number of timelines
    pub timeline_count: usize,
    /// Number of assets
    pub asset_count: usize,
    /// Total duration in frames
    pub total_frames: i64,
}

/// Media file information
#[derive(Debug, Clone)]
pub struct MediaInfo {
    /// File path
    pub path: PathBuf,
    /// File size in bytes
    pub size_bytes: u64,
    /// Media type
    pub media_type: MediaType,
    /// Duration in milliseconds (if applicable)
    pub duration_ms: Option<u64>,
    /// Resolution (width, height) if video/image
    pub resolution: Option<(u32, u32)>,
    /// Frame rate if video
    pub fps: Option<f64>,
    /// Audio channels if audio/video with audio
    pub audio_channels: Option<u32>,
    /// Audio sample rate if audio/video with audio
    pub sample_rate: Option<u32>,
    /// Video codec if video
    pub video_codec: Option<String>,
    /// Audio codec if audio or video with audio
    pub audio_codec: Option<String>,
}

/// Media type detected from file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    /// Video file
    Video,
    /// Audio file
    Audio,
    /// Image file
    Image,
    /// Image sequence
    ImageSequence,
    /// Unknown or unsupported
    Unknown,
}

/// Analyze a media file and return information (stub)
pub fn analyze_media(path: &PathBuf) -> Result<MediaInfo, String> {
    // Check if file exists
    if !path.exists() {
        return Err(format!("File not found: {}", path.display()));
    }

    // Get file size
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let size_bytes = metadata.len();

    // Determine media type from extension
    let media_type = match path.extension().and_then(|e| e.to_str()) {
        Some("mp4") | Some("mov") | Some("avi") | Some("mkv") | Some("webm") => MediaType::Video,
        Some("mp3") | Some("wav") | Some("aac") | Some("flac") | Some("ogg") => MediaType::Audio,
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("bmp") | Some("tiff") => {
            MediaType::Image
        }
        _ => MediaType::Unknown,
    };

    // Stub: Return basic info without actual media probing
    Ok(MediaInfo {
        path: path.clone(),
        size_bytes,
        media_type,
        duration_ms: None,
        resolution: None,
        fps: None,
        audio_channels: None,
        sample_rate: None,
        video_codec: None,
        audio_codec: None,
    })
}

/// Format file size for display
pub fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

/// Format duration for display
pub fn format_duration(ms: u64) -> String {
    let seconds = ms / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;

    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes % 60, seconds % 60)
    } else {
        format!("{}:{:02}", minutes, seconds % 60)
    }
}

/// Print media info in human-readable format
pub fn print_media_info(info: &MediaInfo) {
    println!("File: {}", info.path.display());
    println!("Size: {}", format_file_size(info.size_bytes));
    println!("Type: {:?}", info.media_type);

    if let Some(duration) = info.duration_ms {
        println!("Duration: {}", format_duration(duration));
    }

    if let Some((w, h)) = info.resolution {
        println!("Resolution: {}x{}", w, h);
    }

    if let Some(fps) = info.fps {
        println!("Frame Rate: {:.2} fps", fps);
    }

    if let Some(codec) = &info.video_codec {
        println!("Video Codec: {}", codec);
    }

    if let Some(codec) = &info.audio_codec {
        println!("Audio Codec: {}", codec);
    }

    if let Some(channels) = info.audio_channels {
        println!("Audio Channels: {}", channels);
    }

    if let Some(rate) = info.sample_rate {
        println!("Sample Rate: {} Hz", rate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_file_size() {
        assert_eq!(format_file_size(500), "500 bytes");
        assert_eq!(format_file_size(1024), "1.00 KB");
        assert_eq!(format_file_size(1024 * 1024), "1.00 MB");
        assert_eq!(format_file_size(1024 * 1024 * 1024), "1.00 GB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(30000), "0:30");
        assert_eq!(format_duration(90000), "1:30");
        assert_eq!(format_duration(3661000), "1:01:01");
    }

    #[test]
    fn test_cli_result() {
        let success = CliResult::success("Operation completed");
        assert!(success.success);
        assert_eq!(success.exit_code, 0);

        let failure = CliResult::failure("Operation failed", 1);
        assert!(!failure.success);
        assert_eq!(failure.exit_code, 1);
    }
}
