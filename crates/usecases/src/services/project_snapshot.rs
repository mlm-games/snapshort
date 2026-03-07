use crate::{AppError, AppResult};
use snapshort_domain::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectSnapshot {
    pub schema_version: u32,
    pub project: Project,
    pub assets: Vec<Asset>,
    pub timelines: Vec<Timeline>,
}

impl ProjectSnapshot {
    pub const SCHEMA_VERSION: u32 = 2;

    pub fn new(project: Project, assets: Vec<Asset>, timelines: Vec<Timeline>) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            project,
            assets,
            timelines,
        }
    }
}

pub fn read_snapshot(path: &Path) -> AppResult<ProjectSnapshot> {
    let bytes = std::fs::read(path)?;
    let mut snapshot: ProjectSnapshot = serde_json::from_slice(&bytes)?;
    if snapshot.schema_version != ProjectSnapshot::SCHEMA_VERSION {
        return Err(AppError::InvalidInput(format!(
            "Unsupported project file schema version: {}",
            snapshot.schema_version
        )));
    }

    snapshot.project.path = Some(path.to_path_buf());
    for asset in &mut snapshot.assets {
        asset.path = restore_asset_path(path, &asset.path);
        if let Some(proxy) = &mut asset.proxy {
            proxy.path = restore_asset_path(path, &proxy.path);
        }
    }

    Ok(snapshot)
}

pub fn write_snapshot(path: &Path, snapshot: &ProjectSnapshot) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut snapshot = snapshot.clone();
    snapshot.project.path = None;
    for asset in &mut snapshot.assets {
        asset.path = relativize_path(path, &asset.path);
        if let Some(proxy) = &mut asset.proxy {
            proxy.path = relativize_path(path, &proxy.path);
        }
    }

    let json = serde_json::to_vec_pretty(&snapshot)?;
    std::fs::write(path, json)?;
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
