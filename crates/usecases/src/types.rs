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
    Analyzing,
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
