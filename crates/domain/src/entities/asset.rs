//! Asset entity - represents a media file
use crate::{Fps, Frame, FrameRange, Resolution};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Asset identity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetId(pub Uuid);
impl AssetId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}
impl Default for AssetId {
    fn default() -> Self {
        Self::new()
    }
}
impl std::fmt::Display for AssetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The high-level type of an asset
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssetType {
    Video,
    Audio,
    Image,
    Sequence,
}

/// Current processing/availability status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetStatus {
    Pending,
    Analyzing,
    Ready,
    ProxyGenerating { progress: u8 },
    ProxyReady,
    Error(String),
    Offline,
}
impl AssetStatus {
    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Ready | Self::ProxyReady)
    }
}

/// Codec metadata for streams
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodecInfo {
    pub name: String,                       // e.g. "h264"
    pub profile: String,                    // e.g. "main"
    pub bit_depth: Option<u8>,              // e.g. Some(8)
    pub chroma_subsampling: Option<String>, // e.g. Some("4:2:0")
}
impl CodecInfo {
    pub fn new(name: impl Into<String>, profile: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            profile: profile.into(),
            bit_depth: None,
            chroma_subsampling: None,
        }
    }
}

/// Video stream metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoStream {
    pub codec: CodecInfo,
    pub resolution: Resolution,
    pub fps: Fps,
    pub duration_frames: i64,
    pub pixel_format: String, // e.g. "yuv420p"
    pub color_space: String,  // e.g. "bt709"
    pub hdr: bool,
}

/// Audio stream metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioStream {
    pub codec: CodecInfo,
    pub channels: u16,
    pub sample_rate: u32,
    pub bit_depth: Option<u8>,
    pub duration_samples: u64,
}

/// Media container + streams metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    pub container: String, // e.g. "mp4"
    pub duration_ms: u64,
    pub file_size: u64,
    pub video_streams: Vec<VideoStream>,
    pub audio_streams: Vec<AudioStream>,
}
impl MediaInfo {
    pub fn primary_video(&self) -> Option<&VideoStream> {
        self.video_streams.first()
    }
    pub fn primary_audio(&self) -> Option<&AudioStream> {
        self.audio_streams.first()
    }
    pub fn fps(&self) -> Option<Fps> {
        self.primary_video().map(|v| v.fps)
    }
    pub fn resolution(&self) -> Option<Resolution> {
        self.primary_video().map(|v| v.resolution)
    }
    pub fn duration_frames(&self, fps: Fps) -> i64 {
        self.primary_video()
            .map(|v| v.duration_frames)
            .unwrap_or_else(|| {
                fps.duration_to_frames(std::time::Duration::from_millis(self.duration_ms))
            })
    }
}

/// Proxy (optimized) media info
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProxyInfo {
    pub path: PathBuf,
    pub codec: String,
    pub bitrate_kbps: u32,
    pub resolution: Resolution,
    pub created_at: DateTime<Utc>,
    pub fps: Fps,
}

/// Timeline markers on an asset
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetMarker {
    pub frame: Frame,
    pub label: String,
}

/// Asset entity
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub id: AssetId,
    pub name: String,
    pub path: PathBuf,
    pub asset_type: AssetType,
    pub status: AssetStatus,
    pub media_info: Option<MediaInfo>,
    pub proxy: Option<ProxyInfo>,
    pub imported_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub rating: Option<u8>,
    pub markers: Vec<AssetMarker>,
}

impl Asset {
    pub fn new(path: PathBuf, asset_type: AssetType) -> Self {
        let now = Utc::now();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();
        Self {
            id: AssetId::new(),
            name,
            path,
            asset_type,
            status: AssetStatus::Pending,
            media_info: None,
            proxy: None,
            imported_at: now,
            modified_at: now,
            tags: vec![],
            notes: None,
            rating: None,
            markers: vec![],
        }
    }

    pub fn with_media_info(mut self, info: MediaInfo) -> Self {
        self.media_info = Some(info);
        self
    }

    pub fn source_range(&self) -> Option<FrameRange> {
        self.media_info.as_ref().and_then(|info| {
            info.primary_video()
                .map(|v| FrameRange::new_unchecked(0, v.duration_frames))
        })
    }

    pub fn effective_path(&self) -> &PathBuf {
        if let Some(proxy) = &self.proxy {
            &proxy.path
        } else {
            &self.path
        }
    }

    pub fn is_offline(&self) -> bool {
        matches!(self.status, AssetStatus::Offline)
    }

    /// Update `modified_at` to now (matches `Project::touch()` style).
    pub fn touch(&mut self) {
        self.modified_at = chrono::Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_asset_creation() {
        let asset = Asset::new(PathBuf::from("video.mp4"), AssetType::Video);
        assert_eq!(asset.name, "video.mp4");
        assert!(matches!(asset.status, AssetStatus::Pending));
    }
    #[test]
    fn test_asset_with_proxy_path_preference() {
        let mut asset = Asset::new(PathBuf::from("video.mp4"), AssetType::Video);
        asset.proxy = Some(ProxyInfo {
            path: PathBuf::from("proxies/video_proxy.mp4"),
            codec: "h264".into(),
            bitrate_kbps: 2000,
            resolution: Resolution::HD,
            fps: Fps::default(),
            created_at: chrono::Utc::now(),
        });
        assert_eq!(
            asset.effective_path(),
            &PathBuf::from("proxies/video_proxy.mp4")
        );
    }
}
