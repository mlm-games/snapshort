use crate::{AppError, AppResult, ProjectSnapshot};
use game_utils::save_store::SaveStore;
use game_utils::storage::{FsStorage, Storage};
use std::path::{Path, PathBuf};

pub fn read_snapshot(path: &Path) -> AppResult<ProjectSnapshot> {
    let bytes = FsStorage.read(path)?.ok_or_else(|| {
        AppError::InvalidInput(format!("Project file not found: {}", path.display()))
    })?;
    let mut snapshot: ProjectSnapshot = serde_json::from_slice(&bytes)?;
    if snapshot.schema_version > ProjectSnapshot::SCHEMA_VERSION {
        return Err(AppError::InvalidInput(format!(
            "Unsupported project file schema version: {}",
            snapshot.schema_version
        )));
    }
    snapshot.schema_version = ProjectSnapshot::SCHEMA_VERSION;

    for asset in &mut snapshot.assets {
        asset.path = restore_asset_path(path, &asset.path);
        if let Some(proxy) = &mut asset.proxy {
            proxy.path = restore_asset_path(path, &proxy.path);
        }
    }

    Ok(snapshot)
}

pub fn write_snapshot(path: &Path, snapshot: &ProjectSnapshot) -> AppResult<()> {
    let mut snapshot = snapshot.clone();
    for asset in &mut snapshot.assets {
        asset.path = relativize_path(path, &asset.path);
        if let Some(proxy) = &mut asset.proxy {
            proxy.path = relativize_path(path, &proxy.path);
        }
    }

    let json = serde_json::to_vec_pretty(&snapshot)?;
    // Crash-safe write: temp file + atomic rename with `.bak` rotation, and
    // corrupt leftovers quarantined instead of overwritten (SaveStore model).
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::InvalidInput(format!("Invalid path: {}", path.display())))?;
    SaveStore::new_with_storage(dir, file_name.to_string(), FsStorage)
        .write(&json)
        .map_err(AppError::Other)?;
    Ok(())
}

fn relativize_path(project_path: &Path, path: &Path) -> PathBuf {
    let Some(base_dir) = project_path.parent() else {
        return path.to_path_buf();
    };
    path.strip_prefix(base_dir)
        .map(|relative| relative.to_path_buf())
        .unwrap_or_else(|_| path.to_path_buf())
}

fn restore_asset_path(project_path: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    project_path
        .parent()
        .map(|parent| parent.join(path))
        .unwrap_or_else(|| path.to_path_buf())
}
