//! Project entity - top-level container

use crate::{AssetId, TimelineId, Fps, Resolution};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectId(pub Uuid);

impl ProjectId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ProjectId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub fps: Fps,
    pub resolution: Resolution,
    pub sample_rate: u32,
    pub proxy_resolution: Resolution,
    pub cache_dir: Option<PathBuf>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            fps: Fps::F24,
            resolution: Resolution::HD,
            sample_rate: 48000,
            proxy_resolution: Resolution::new(1280, 720),
            cache_dir: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub path: Option<PathBuf>,
    pub settings: ProjectSettings,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub asset_ids: Vec<AssetId>,
    pub timeline_ids: Vec<TimelineId>,
    pub active_timeline_id: Option<TimelineId>,
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: ProjectId::new(),
            name: name.into(),
            path: None,
            settings: ProjectSettings::default(),
            created_at: now,
            modified_at: now,
            asset_ids: Vec::new(),
            timeline_ids: Vec::new(),
            active_timeline_id: None,
        }
    }

    pub fn touch(&mut self) {
        self.modified_at = Utc::now();
    }
}
