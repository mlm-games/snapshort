use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AssetId(pub uuid::Uuid);

impl AssetId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl std::fmt::Display for AssetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AssetType {
    Video,
    Audio,
    Image,
    Sequence,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AssetStatus {
    Pending,
    Analyzing { progress: u8 },
    Ready,
    ProxyReady,
    Offline,
    ProxyGenerating { progress: u8 },
    Error(String),
}

impl AssetStatus {
    pub fn is_usable(&self) -> bool {
        matches!(self, AssetStatus::Ready | AssetStatus::ProxyReady)
    }

    pub fn is_error(&self) -> bool {
        matches!(self, AssetStatus::Error(_))
    }

    pub fn error_message(&self) -> Option<&str> {
        match self {
            AssetStatus::Error(msg) => Some(msg),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Marker {
    pub frame: i64,
    pub label: String,
    pub color: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Asset {
    pub id: AssetId,
    pub name: String,
    pub path: PathBuf,
    pub asset_type: AssetType,
    pub status: AssetStatus,
    pub media_info: Option<snapshort_infra_media::MediaInfo>,
    pub proxy: Option<snapshort_infra_media::ProxyInfo>,
    pub imported_at: chrono::DateTime<chrono::Utc>,
    pub modified_at: chrono::DateTime<chrono::Utc>,
    pub tags: Vec<String>,
    pub notes: String,
    pub rating: Option<u8>,
    pub markers: Vec<Marker>,
}

impl Asset {
    pub fn new(path: PathBuf, asset_type: AssetType) -> Self {
        let now = chrono::Utc::now();
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
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
            tags: Vec::new(),
            notes: String::new(),
            rating: None,
            markers: Vec::new(),
        }
    }

    pub fn effective_path(&self) -> &PathBuf {
        self.proxy.as_ref().map(|p| &p.path).unwrap_or(&self.path)
    }

    pub fn touch(&mut self) {
        self.modified_at = chrono::Utc::now();
    }
}

/// Proxy generation policy: when the pipeline creates lightweight stand-ins
/// automatically. Proxies speed up preview (and thumbnails); export always
/// uses full-resolution sources.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProxyPolicy {
    /// Auto-submit proxy jobs after analysis.
    pub auto_generate: bool,
    /// Generate when the primary video width is at or above this (pixels).
    pub min_width: u32,
}

impl Default for ProxyPolicy {
    fn default() -> Self {
        Self {
            auto_generate: true,
            min_width: 1920,
        }
    }
}

/// Pure auto-proxy decision: video at/above the threshold, policy on, and no
/// proxy yet. Headless-testable; the service only executes the verdict.
pub fn should_auto_proxy(policy: &ProxyPolicy, asset: &Asset) -> bool {
    if !policy.auto_generate || asset.asset_type != AssetType::Video || asset.proxy.is_some() {
        return false;
    }
    asset
        .media_info
        .as_ref()
        .and_then(|info| info.primary_video())
        .map(|stream| stream.width >= policy.min_width)
        .unwrap_or(false)
}

/// A timeline marker for save/open round-trips. Lives in types (not services)
/// so the wasm shell can build Save/SaveAs commands without the native backend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TimelineMarkerData {
    pub timestamp_us: i64,
    pub label: String,
}

/// Versioned project file. Lives in types so every platform shell can
/// serialize/parse snapshots; only the file IO stays in services.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectSnapshot {
    pub schema_version: u32,
    pub project: miniter_domain::Project,
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub timeline_markers: Vec<TimelineMarkerData>,
}

impl ProjectSnapshot {
    pub const SCHEMA_VERSION: u32 = 4;

    pub fn new(
        project: miniter_domain::Project,
        assets: Vec<Asset>,
        timeline_markers: Vec<TimelineMarkerData>,
    ) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            project,
            assets,
            timeline_markers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snapshort_infra_media::{MediaInfo, ProxyInfo, VideoStream};

    fn video_asset(width: u32) -> Asset {
        let mut asset = Asset::new(
            std::path::PathBuf::from("/tmp/clip.mp4"),
            AssetType::Video,
        );
        asset.media_info = Some(MediaInfo {
            container: "mp4".into(),
            duration_ms: 10_000,
            file_size: 1_000_000,
            video_streams: vec![VideoStream {
                codec_name: "h264".into(),
                codec_profile: "high".into(),
                bit_depth: Some(8),
                chroma_subsampling: Some("4:2:0".into()),
                width,
                height: 1080,
                fps: 30.0,
                duration_frames: 300,
                pixel_format: "yuv420p".into(),
                color_space: "bt709".into(),
                hdr: false,
            }],
            audio_streams: vec![],
            waveform: None,
        });
        asset
    }

    #[test]
    fn proxy_policy_triggers_only_for_qualifying_video() {
        let policy = ProxyPolicy::default();
        assert!(policy.auto_generate);

        // At/above threshold with no proxy: generate.
        assert!(should_auto_proxy(&policy, &video_asset(1920)));
        assert!(should_auto_proxy(&policy, &video_asset(3840)));

        // Below threshold: skip.
        assert!(!should_auto_proxy(&policy, &video_asset(1280)));

        // Policy off: skip everything.
        let off = ProxyPolicy {
            auto_generate: false,
            min_width: 1,
        };
        assert!(!should_auto_proxy(&off, &video_asset(7680)));

        // Non-video never qualifies…
        let mut audio = video_asset(3840);
        audio.asset_type = AssetType::Audio;
        assert!(!should_auto_proxy(&policy, &audio));

        // …nor analyzed assets without stream info…
        let mut unknown = video_asset(3840);
        unknown.media_info = None;
        assert!(!should_auto_proxy(&policy, &unknown));

        // …nor assets that already have a proxy.
        let mut proxied = video_asset(3840);
        proxied.proxy = Some(ProxyInfo {
            path: std::path::PathBuf::from("/tmp/proxy.mp4"),
            codec: "h264".into(),
            bitrate_kbps: 2000,
            fps: 30.0,
            width: 960,
            height: 540,
            created_at: chrono::Utc::now(),
        });
        assert!(!should_auto_proxy(&policy, &proxied));
    }
}
