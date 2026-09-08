use crate::{parse_snapshot_bytes, AppError, AppResult, ProjectSnapshot, StrippedSummary};
use game_utils::save_store::SaveStore;
use game_utils::storage::{FsStorage, Storage};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Crash-recovery autosave: a single shadow copy of the open project under
/// the app data dir (never next to user files). Mirrors the web `autosave.json`
/// model; restored only through the recovery prompt, never silently.
pub const AUTOSAVE_SNAP_NAME: &str = "autosave.snap";
pub const AUTOSAVE_META_NAME: &str = "autosave.json";
/// Timestamped copies kept next to the project file on explicit save.
pub const BACKUP_DIR_NAME: &str = ".snapshort-backups";
pub const BACKUP_KEEP: usize = 5;
/// Autosave cadence (matches the web backend).
pub const AUTOSAVE_INTERVAL_SECS: u64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutosaveMeta {
    /// Where the project was last explicitly saved (`None` = never saved).
    pub project_path: Option<PathBuf>,
    pub project_name: String,
    /// Unix millis when the autosave was written.
    pub saved_at_ms: i64,
}

pub fn now_ms() -> i64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `<data_dir>/autosave/` paths (snapshot + sidecar metadata).
pub fn autosave_paths(data_dir: &Path) -> (PathBuf, PathBuf) {
    let dir = data_dir.join("autosave");
    (dir.join(AUTOSAVE_SNAP_NAME), dir.join(AUTOSAVE_META_NAME))
}

pub fn read_snapshot(path: &Path) -> AppResult<ProjectSnapshot> {
    Ok(read_snapshot_report(path)?.0)
}

/// Open a project file, tolerating effects written by newer builds.
/// Unknown filter/mask/transition/clip variants are stripped (counted in the
/// summary) instead of failing the whole open; media, cuts, and known
/// effects load untouched. Callers surface a non-empty summary as a toast —
/// stripping mutates the in-memory project, never the file on disk.
pub fn read_snapshot_report(path: &Path) -> AppResult<(ProjectSnapshot, StrippedSummary)> {
    let bytes = FsStorage.read(path)?.ok_or_else(|| {
        AppError::InvalidInput(format!("Project file not found: {}", path.display()))
    })?;
    let (mut snapshot, summary) = parse_snapshot_bytes(&bytes)?;

    for asset in &mut snapshot.assets {
        asset.path = restore_asset_path(path, &asset.path);
        if let Some(proxy) = &mut asset.proxy {
            proxy.path = restore_asset_path(path, &proxy.path);
        }
    }

    Ok((snapshot, summary))
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
    rotate_backups(path);
    Ok(())
}

/// Copy the just-saved file into `.snapshort-backups/` next to it, pruning to
/// [`BACKUP_KEEP`]. Best-effort: backup failures never fail the save itself.
fn rotate_backups(path: &Path) {
    let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) else {
        return;
    };
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return;
    };
    let backup_dir = parent.join(BACKUP_DIR_NAME);
    if FsStorage.create_dir_all(&backup_dir).is_err() {
        return;
    }
    let dest = backup_dir.join(format!("{:016}_{stem}.snap", now_ms().max(0)));
    // Copy (not rename): the live file stays in place; failures are silent.
    if FsStorage.copy(path, &dest).is_err() {
        return;
    }
    FsStorage.sync_file(&dest);
    FsStorage.sync_dir(&backup_dir);

    let mut backups: Vec<(u64, PathBuf)> = Vec::new();
    let entries = FsStorage.read_dir(&backup_dir).unwrap_or_default();
    for entry in entries {
        let is_snap = entry.extension().and_then(|e| e.to_str()) == Some("snap");
        if !is_snap || !FsStorage.is_file(&entry) {
            continue;
        }
        // Sort key: mtime, tie-broken by name (millis-prefixed names sort
        // chronologically anyway).
        backups.push((FsStorage.mtime_secs(&entry).unwrap_or(0), entry));
    }
    backups.sort();
    while backups.len() > BACKUP_KEEP {
        let (_, oldest) = backups.remove(0);
        let _ = FsStorage.remove_file(&oldest);
    }
    FsStorage.sync_dir(&backup_dir);
}

/// Write a crash-recovery shadow copy (full snapshot + metadata sidecar).
/// Atomic like any save; overwrites the previous autosave.
pub fn write_autosave(
    data_dir: &Path,
    snapshot: &ProjectSnapshot,
    meta: &AutosaveMeta,
) -> AppResult<()> {
    let (snap_path, meta_path) = autosave_paths(data_dir);
    let json = serde_json::to_vec_pretty(snapshot)?;
    let meta_json = serde_json::to_vec_pretty(meta)?;
    let Some(dir) = snap_path.parent() else {
        return Err(AppError::InvalidInput("Invalid autosave dir".into()));
    };
    let (Some(snap_name), Some(meta_name)) = (
        snap_path.file_name().and_then(|n| n.to_str()),
        meta_path.file_name().and_then(|n| n.to_str()),
    ) else {
        return Err(AppError::InvalidInput("Invalid autosave names".into()));
    };
    SaveStore::new_with_storage(dir.to_path_buf(), snap_name.to_string(), FsStorage)
        .write(&json)
        .map_err(AppError::Other)?;
    SaveStore::new_with_storage(dir.to_path_buf(), meta_name.to_string(), FsStorage)
        .write(&meta_json)
        .map_err(AppError::Other)?;
    Ok(())
}

/// Read back the autosave sidecar, if any.
pub fn read_autosave_meta(data_dir: &Path) -> Option<AutosaveMeta> {
    let (_, meta_path) = autosave_paths(data_dir);
    let bytes = FsStorage.read(&meta_path).ok()??;
    serde_json::from_slice(&bytes).ok()
}

/// Read back the autosave snapshot, validated like any project file.
/// Absence is normal (no crash pending); corruption is warned — without
/// this, a damaged shadow copy would silently skip crash recovery.
pub fn read_autosave_snapshot(data_dir: &Path) -> Option<ProjectSnapshot> {
    let (snap_path, _) = autosave_paths(data_dir);
    if FsStorage.read(&snap_path).ok()??.is_empty() {
        return None;
    }
    match read_snapshot(&snap_path) {
        Ok(snapshot) => Some(snapshot),
        Err(e) => {
            tracing::warn!(
                "Ignoring corrupt autosave {}: {e}",
                snap_path.display()
            );
            None
        }
    }
}

/// Delete the shadow copy (after restore, explicit save, or user discard).
pub fn clear_autosave(data_dir: &Path) {
    let (snap_path, meta_path) = autosave_paths(data_dir);
    for path in [&snap_path, &meta_path] {
        let _ = FsStorage.remove_file(path);
        // Remove SaveStore siblings too so a deleted autosave can't resurrect.
        if let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) {
            SaveStore::new_with_storage(dir.to_path_buf(), name.to_string(), FsStorage).delete();
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn golden_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/project-v4.snap")
    }

    #[test]
    fn golden_file_parses_with_expected_shape() {
        let raw = std::fs::read_to_string(golden_path()).expect("golden file checked in");
        // The checked-in file pins the CURRENT schema: bumping
        // SCHEMA_VERSION requires regenerating it (see zz generator below).
        assert!(
            raw.contains("\"schema_version\": 4"),
            "golden must pin schema 4"
        );
        assert_eq!(ProjectSnapshot::SCHEMA_VERSION, 4);

        // read_snapshot resolves the relative asset path against testdata/.
        let snapshot = read_snapshot(&golden_path()).expect("golden parses");
        assert_eq!(snapshot.project.meta.name, "Golden");
        assert_eq!(snapshot.project.timeline.tracks.len(), 1);
        assert_eq!(snapshot.assets.len(), 1);
        assert_eq!(snapshot.assets[0].name, "clip");
        assert!(snapshot.assets[0]
            .path
            .to_string_lossy()
            .ends_with("media/clip.mp4"));
        assert_eq!(snapshot.timeline_markers.len(), 1);

        // Roundtrip stability: parse → serialize → parse yields the same data.
        let reserialized = serde_json::to_vec_pretty(&snapshot).unwrap();
        let reparsed: ProjectSnapshot = serde_json::from_slice(&reserialized).unwrap();
        assert_eq!(
            serde_json::to_value(&reparsed).unwrap(),
            serde_json::to_value(&snapshot).unwrap()
        );
    }

    #[test]
    fn unreadable_project_files_are_rejected() {
        // Missing file…
        let err = read_snapshot(Path::new("/tmp/snapshort-test-missing-xyz.snap")).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));

        let dir = tempfile::tempdir().unwrap();
        // …truncated JSON…
        let corrupt = dir.path().join("corrupt.snap");
        std::fs::write(&corrupt, r#"{"schema_version": 4,"project": {"#).unwrap();
        assert!(read_snapshot(&corrupt).is_err());

        // …and valid files from the future (bump the golden copy).
        let raw = std::fs::read_to_string(golden_path()).expect("golden file checked in");
        let bumped = raw.replacen("\"schema_version\": 4", "\"schema_version\": 999", 1);
        assert_ne!(bumped, raw);
        let future = dir.path().join("future.snap");
        std::fs::write(&future, bumped).unwrap();
        let err = read_snapshot(&future).unwrap_err();
        assert!(
            matches!(err, AppError::InvalidInput(_)),
            "expected InvalidInput, got {err:?}"
        );
    }

    #[test]
    fn saves_rotate_backups_capped() {
        use crate::types::Asset;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.snap");
        for i in 0..7 {
            let mut project = miniter_domain::Project::new(format!("v{i}"));
            project.meta.modified_at = 1_700_000_000_000 + i;
            let snapshot = ProjectSnapshot::new(project, Vec::<Asset>::new(), vec![]);
            write_snapshot(&path, &snapshot).unwrap();
        }
        // Live file holds the last save; backups are capped.
        let live: ProjectSnapshot =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(live.project.meta.name, "v6");
        let backup_dir = dir.path().join(BACKUP_DIR_NAME);
        let mut backups: Vec<_> = std::fs::read_dir(&backup_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert!(
            backups.len() <= BACKUP_KEEP,
            "expected <= {BACKUP_KEEP} backups, found {}",
            backups.len()
        );
        assert!(!backups.is_empty(), "expected at least one backup");
        backups.sort();
        // Oldest retained backup parses (quarantine never eats valid saves).
        let oldest: ProjectSnapshot =
            serde_json::from_slice(&std::fs::read(&backups[0]).unwrap()).unwrap();
        assert!(oldest.project.meta.name.starts_with('v'));
    }

    #[test]
    fn autosave_roundtrips_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path();
        assert!(read_autosave_meta(data_dir).is_none());
        assert!(read_autosave_snapshot(data_dir).is_none());

        let project = miniter_domain::Project::new("Work");
        let snapshot = ProjectSnapshot::new(project, vec![], vec![]);
        write_autosave(
            data_dir,
            &snapshot,
            &AutosaveMeta {
                project_path: None,
                project_name: "Work".into(),
                saved_at_ms: now_ms(),
            },
        )
        .unwrap();

        let meta = read_autosave_meta(data_dir).expect("meta readable");
        assert_eq!(meta.project_name, "Work");
        assert!(meta.project_path.is_none());
        let back = read_autosave_snapshot(data_dir).expect("snapshot readable");
        assert_eq!(back.project.meta.name, "Work");

        clear_autosave(data_dir);
        assert!(read_autosave_meta(data_dir).is_none());
        assert!(read_autosave_snapshot(data_dir).is_none());
    }

    #[test]
    fn corrupt_autosave_reads_as_absent_not_panic() {
        // A damaged shadow copy must never break the boot check: absence
        // and corruption both read as None (corruption additionally warns).
        let dir = tempfile::tempdir().unwrap();
        let (snap_path, _) = autosave_paths(dir.path());
        std::fs::create_dir_all(snap_path.parent().unwrap()).unwrap();
        std::fs::write(&snap_path, b"{\"schema_version\": 4, \"project\": {").unwrap();
        assert!(read_autosave_snapshot(dir.path()).is_none());
    }

    #[test]
    fn read_snapshot_report_opens_future_files_with_counts() {
        use crate::forward_compat::forward_compat_tests::future_file;
        let dir = tempfile::tempdir().unwrap();
        let path = future_file(dir.path());
        // Report variant opens what the strict typed parse rejects…
        let (snapshot, summary) = read_snapshot_report(&path).unwrap();
        assert_eq!(summary.total(), 5);
        assert_eq!(snapshot.project.meta.name, "Future");
    }
}
