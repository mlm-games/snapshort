use crate::{AppError, AppResult, Asset};
use miniter_domain::Project;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TimelineMarkerData {
    pub timestamp_us: i64,
    pub label: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectSnapshot {
    pub schema_version: u32,
    pub project: Project,
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub timeline_markers: Vec<TimelineMarkerData>,
}

impl ProjectSnapshot {
    pub const SCHEMA_VERSION: u32 = 4;

    pub fn new(project: Project, assets: Vec<Asset>, timeline_markers: Vec<TimelineMarkerData>) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            project,
            assets,
            timeline_markers,
        }
    }
}

pub fn read_snapshot(path: &Path) -> AppResult<ProjectSnapshot> {
    let bytes = std::fs::read(path)?;
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
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut snapshot = snapshot.clone();
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
