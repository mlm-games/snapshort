//! Infrastructure layer - media probing + proxy generation (stub)
//!
//! This crate is intentionally minimal right now. It compiles and provides
//! a usable stub for later wiring.

use snapshort_domain::{
    AudioStream, CodecInfo, Fps, MediaInfo, ProxyInfo, Resolution, VideoStream,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct MediaEngine;

impl MediaEngine {
    pub fn probe(&self, path: &Path) -> MediaInfo {
        let container = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_else(|| "unknown".to_string());

        let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

        MediaInfo {
            container,
            duration_ms: 10_000,
            file_size,
            video_streams: vec![VideoStream {
                codec: CodecInfo {
                    name: "h264".to_string(),
                    profile: "main".to_string(),
                    bit_depth: Some(8),
                    chroma_subsampling: Some("4:2:0".to_string()),
                },
                resolution: Resolution::HD,
                fps: Fps::F24,
                duration_frames: 240,
                pixel_format: "yuv420p".to_string(),
                color_space: "bt709".to_string(),
                hdr: false,
            }],
            audio_streams: vec![AudioStream {
                codec: CodecInfo {
                    name: "aac".to_string(),
                    profile: "lc".to_string(),
                    bit_depth: Some(16),
                    chroma_subsampling: None,
                },
                channels: 2,
                sample_rate: 48_000,
                bit_depth: Some(16),
                duration_samples: 0,
            }],
        }
    }

    pub fn create_proxy_placeholder(
        &self,
        asset_id: uuid::Uuid,
        out_dir: &Path,
    ) -> std::io::Result<ProxyInfo> {
        std::fs::create_dir_all(out_dir)?;
        let out_path = out_dir.join(format!("{}_proxy.mp4", asset_id));
        std::fs::write(&out_path, b"proxy placeholder")?;

        Ok(ProxyInfo {
            path: out_path,
            codec: "h264".to_string(),
            bitrate_kbps: 2000,
            resolution: Resolution::HD,
            created_at: chrono::Utc::now(),
        })
    }
}
