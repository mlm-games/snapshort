//! CLI library: small shared pieces for the `snapshort-cli` binary.
//!
//! The binary itself is a thin shell over [`snapshort_engine::Session`] and
//! [`snapshort_automation`]; this module holds the output helpers and the
//! media probe the CLI exposes as `probe`.

use std::path::{Path, PathBuf};

/// Result of a CLI operation.
#[derive(Debug)]
pub struct CliResult {
    pub success: bool,
    pub message: String,
    pub exit_code: i32,
}

impl CliResult {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            exit_code: 0,
        }
    }

    pub fn failure(message: impl Into<String>, exit_code: i32) -> Self {
        Self {
            success: false,
            message: message.into(),
            exit_code,
        }
    }
}

/// Probe one media file as JSON, or an error string.
pub fn probe_media(path: &Path) -> Result<serde_json::Value, String> {
    let engine = snapshort_infra_media::MediaEngine;
    engine
        .probe(path)
        .map(|info| snapshort_engine::media_info_json(&info))
        .map_err(|e| e.to_string())
}

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
        format!("{bytes} bytes")
    }
}

pub fn format_duration(ms: i64) -> String {
    let ms = ms.max(0);
    let seconds = ms / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;

    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes % 60, seconds % 60)
    } else {
        format!("{}:{:02}", minutes, seconds % 60)
    }
}

pub fn print_probe(path: &Path, value: &serde_json::Value) {
    println!("File: {}", path.display());
    println!(
        "Size: {}",
        format_file_size(value["fileSize"].as_u64().unwrap_or(0))
    );
    println!("Container: {}", value["container"].as_str().unwrap_or("-"));
    if let Some(duration) = value["durationMs"].as_i64() {
        println!("Duration: {}", format_duration(duration));
    }
    if let Some(video) = value["video"].as_object() {
        println!(
            "Video: {} {}x{} @ {:.3} fps, {}, {} bit",
            video["codec"].as_str().unwrap_or("-"),
            video["width"].as_u64().unwrap_or(0),
            video["height"].as_u64().unwrap_or(0),
            video["fps"].as_f64().unwrap_or(0.0),
            video["pixelFormat"].as_str().unwrap_or("-"),
            video["colorSpace"].as_str().unwrap_or("-"),
        );
    }
    if let Some(audio) = value["audio"].as_object() {
        println!(
            "Audio: {} {} Hz, {} ch",
            audio["codec"].as_str().unwrap_or("-"),
            audio["sampleRate"].as_u64().unwrap_or(0),
            audio["channels"].as_u64().unwrap_or(0),
        );
    }
}

/// The app data directory: `--data-dir` when given, else the OS location.
pub fn data_dir(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = explicit {
        return dir;
    }
    #[cfg(not(target_os = "android"))]
    {
        directories::ProjectDirs::from("com", "mlm-games", "snapshort")
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".").join(".snapshort"))
    }
    #[cfg(target_os = "android")]
    PathBuf::from(".")
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
        let success = CliResult::success("done");
        assert!(success.success);
        assert_eq!(success.exit_code, 0);

        let failure = CliResult::failure("failed", 1);
        assert!(!failure.success);
        assert_eq!(failure.exit_code, 1);
    }
}
