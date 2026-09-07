#![cfg(target_arch = "wasm32")]
//! Browser persistence for the web shell (yadaw `wasm_persist.rs` pattern).
//!
//! OPFS solves *file* persistence (project snapshots, autosave, recents) —
//! the layer that matters for a working web editor. It does not, and cannot,
//! provide sqlite: the wasm backend is in-memory by design (`backend_wasm`).

pub mod keys {
    pub const DIR_PROJECTS: &str = "projects";
    pub const FILE_AUTOSAVE: &str = "autosave.json";
    pub const FILE_RECENT: &str = "recent.json";

    pub fn project_key(name: &str) -> String {
        format!("{DIR_PROJECTS}/{name}")
    }
}

/// Create the OPFS project directories. Call once at startup.
pub async fn init() -> Result<(), String> {
    opfs::ensure_dir(keys::DIR_PROJECTS)
        .await
        .map_err(|e| format!("OPFS init: {e}"))?;
    Ok(())
}

/// Write a project snapshot to browser storage.
pub async fn save_project(name: &str, data: &[u8]) -> Result<(), String> {
    let key = keys::project_key(name);
    opfs::write(&key, data)
        .await
        .map_err(|e| format!("OPFS write {key}: {e}"))?;
    remember_recent(name).await;
    Ok(())
}

/// Write the autosave snapshot.
pub async fn save_autosave(data: &[u8]) -> Result<(), String> {
    opfs::write(keys::FILE_AUTOSAVE, data)
        .await
        .map_err(|e| format!("OPFS autosave: {e}"))
}

/// Read the autosave snapshot, if any.
pub async fn load_autosave() -> Option<Vec<u8>> {
    opfs::read(keys::FILE_AUTOSAVE).await.ok()
}

/// Recent project names, most recent first.
pub async fn load_recent() -> Vec<String> {
    opfs::read(keys::FILE_RECENT)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

async fn remember_recent(name: &str) {
    let mut recent = load_recent().await;
    recent.retain(|n| n != name);
    recent.insert(0, name.to_string());
    recent.truncate(8);
    if let Ok(bytes) = serde_json::to_vec(&recent) {
        let _ = opfs::write(keys::FILE_RECENT, &bytes).await;
    }
}
