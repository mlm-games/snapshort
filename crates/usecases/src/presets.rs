//! Export presets: the settings a render is built from, by name.
//!
//! Built-ins cover the common destinations; the user's own live next to the
//! other app data as JSON, so they survive reinstalls the way projects do.
//! Writes go through `game-utils`' crash-safe store, which is the filesystem
//! on desktop and OPFS on web.

use std::path::Path;

use game_utils::save_store::SaveStore;
use game_utils::storage::FsStorage;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const PRESET_FILE: &str = "export-presets.json";

/// One named render recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    /// `mp4`, `mkv`, `mov`, `av1-mp4`, `av1-mkv`, `av1-webm`, `av1-ivf`,
    /// `opus`, `flac`.
    pub format: String,
    /// `draft`, `preview`, `standard`, `high`, `master`.
    pub quality: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// 0 = the format's default.
    pub video_bitrate: u32,
    pub audio_bitrate: u32,
    pub hardware_accel: bool,
}

impl Preset {
    fn builtin(name: &str, format: &str, quality: &str, width: u32, height: u32, fps: f64) -> Self {
        Self {
            name: name.to_string(),
            format: format.to_string(),
            quality: quality.to_string(),
            width,
            height,
            fps,
            video_bitrate: 0,
            audio_bitrate: 0,
            hardware_accel: true,
        }
    }
}

/// The recipes shipped with the app.
pub fn builtin_presets() -> Vec<Preset> {
    vec![
        Preset::builtin("YouTube 1080p", "mp4", "high", 1920, 1080, 30.0),
        Preset::builtin("YouTube 4K", "mp4", "master", 3840, 2160, 30.0),
        Preset::builtin("Vertical", "mp4", "high", 1080, 1920, 30.0),
        Preset::builtin("Square", "mp4", "high", 1080, 1080, 30.0),
        Preset::builtin("Web AV1", "av1-webm", "standard", 1920, 1080, 30.0),
        Preset::builtin("Master MOV", "mov", "master", 1920, 1080, 24.0),
        Preset::builtin("Preview", "mp4", "preview", 960, 540, 24.0),
    ]
}

fn store(data_dir: &Path) -> SaveStore<FsStorage> {
    SaveStore::new(data_dir, PRESET_FILE)
}

/// The user's presets, newest first.
pub fn load_user_presets_in(data_dir: &Path) -> Vec<Preset> {
    let loaded = store(data_dir).load(&SaveStore::<FsStorage>::is_intact_json, &[]);
    loaded
        .data
        .and_then(|bytes| serde_json::from_slice::<Vec<Preset>>(&bytes).ok())
        .unwrap_or_default()
}

pub fn save_user_presets(data_dir: &Path, presets: &[Preset]) -> Result<(), String> {
    let body = serde_json::to_vec_pretty(presets).map_err(|e| format!("presets: {e}"))?;
    store(data_dir).write(&body)
}

/// Every preset, the user's first (they shadow a built-in of the same name).
pub fn all_presets_in(data_dir: &Path) -> Vec<Preset> {
    let mut user = load_user_presets_in(data_dir);
    let builtin: Vec<Preset> = builtin_presets()
        .into_iter()
        .filter(|b| !user.iter().any(|u| u.name == b.name))
        .collect();
    user.extend(builtin);
    user
}

pub fn find_preset(data_dir: &Path, name: &str) -> Option<Preset> {
    all_presets_in(data_dir)
        .into_iter()
        .find(|p| p.name == name)
}

/// Preset names and their shapes, for the UI and the control channel.
pub fn preset_names(data_dir: &Path) -> Vec<serde_json::Value> {
    all_presets_in(data_dir)
        .iter()
        .map(|preset| {
            json!({
                "name": preset.name,
                "format": preset.format,
                "quality": preset.quality,
                "width": preset.width,
                "height": preset.height,
            })
        })
        .collect()
}
