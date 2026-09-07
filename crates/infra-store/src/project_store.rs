//! File-backed project library on [`Storage`](game_utils::storage::Storage).
//!
//! Layout under the store root:
//!
//! ```text
//! <root>/projects/<project-id>.json   # `Project` pretty JSON (upsert)
//! ```
//!
//! Every write goes through `SaveStore` (temp file + atomic rename, `.bak`
//! rotation, corrupt quarantine), so a crash mid-save never loses the previous
//! intact revision. Listing is a dir scan ordered by `modified_at` desc — the
//! file equivalent of the old `SELECT ... ORDER BY modified_at DESC`.

use crate::{StoreError, StoreResult};
use game_utils::save_store::SaveStore;
use game_utils::storage::{FsStorage, Storage};
use miniter_domain::{Project, ProjectId};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ProjectStore<S: Storage = FsStorage> {
    dir: PathBuf,
    storage: S,
}

impl ProjectStore<FsStorage> {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::new_with_storage(root, FsStorage)
    }
}

impl<S: Storage> ProjectStore<S> {
    pub fn new_with_storage(root: impl Into<PathBuf>, storage: S) -> Self {
        let dir = root.into().join("projects");
        // Best-effort: surfaces as IO errors on first write if it fails here
        // (e.g. Android storage not ready yet).
        let _ = storage.create_dir_all(&dir);
        Self { dir, storage }
    }

    fn slot(&self, file_name: &str) -> SaveStore<S> {
        SaveStore::new_with_storage(self.dir.clone(), file_name.to_string(), self.storage.clone())
    }

    fn file_name(id: ProjectId) -> String {
        format!("{}.json", id.0)
    }

    /// Insert or replace the project (upsert). Re-saving the same project is
    /// the normal path — unlike the old sqlite `INSERT`, this never fails
    /// with a primary-key conflict.
    pub fn save(&self, project: &Project) -> StoreResult<()> {
        let json = project.to_json()?;
        self.slot(&Self::file_name(project.id))
            .write(json.as_bytes())
            .map_err(StoreError::Io)?;
        Ok(())
    }

    /// Load one project, recovering from `.bak`/`temp_` on corruption like any
    /// `SaveStore` load.
    pub fn get(&self, id: ProjectId) -> StoreResult<Option<Project>> {
        let loaded = self
            .slot(&Self::file_name(id))
            .load(&SaveStore::<S>::is_intact_json, &[]);
        let bytes = match loaded.data {
            Some(b) => b,
            None => return Ok(None),
        };
        let text = std::str::from_utf8(&bytes)
            .map_err(|e| StoreError::Constraint(format!("Invalid UTF-8 in project file: {e}")))?;
        Ok(Some(Project::from_json(text)?))
    }

    /// All stored projects, most recently modified first. Corrupt entries are
    /// skipped (they stay quarantined on disk for support); missing dirs read
    /// as empty.
    pub fn list(&self) -> StoreResult<Vec<Project>> {
        let entries = match self.storage.read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut projects = Vec::new();
        for path in entries {
            if !is_project_file(&path) {
                continue;
            }
            let Ok(Some(bytes)) = self.storage.read(&path) else {
                continue;
            };
            let Ok(text) = std::str::from_utf8(&bytes) else {
                continue;
            };
            if let Ok(project) = Project::from_json(text) {
                projects.push(project);
            }
        }
        projects.sort_by(|a, b| b.meta.modified_at.cmp(&a.meta.modified_at));
        Ok(projects)
    }

    /// Idempotent delete: removes the project file together with its
    /// `.bak`/`temp_` siblings so a later load can't resurrect it.
    pub fn delete(&self, id: ProjectId) {
        self.slot(&Self::file_name(id)).delete();
    }
}

fn is_project_file(path: &std::path::Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.ends_with(".json") && !name.starts_with("temp_") && !name.starts_with("corrupted_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_utils::storage::MemoryStorage;

    fn store() -> ProjectStore<MemoryStorage> {
        ProjectStore::new_with_storage("/test-store", MemoryStorage::new())
    }

    #[test]
    fn roundtrip_save_get() {
        let store = store();
        let project = Project::new("Test Project");
        store.save(&project).unwrap();
        let loaded = store.get(project.id).unwrap().unwrap();
        assert_eq!(loaded.meta.name, "Test Project");
        assert_eq!(loaded.id.0, project.id.0);
    }

    #[test]
    fn save_is_upsert_not_insert() {
        // The old sqlite backend failed here with a primary-key conflict.
        let store = store();
        let mut project = Project::new("v1");
        store.save(&project).unwrap();
        project.meta.name = "v2".to_string();
        project.meta.modified_at += 1;
        store.save(&project).unwrap();
        let loaded = store.get(project.id).unwrap().unwrap();
        assert_eq!(loaded.meta.name, "v2");
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn list_orders_by_modified_desc() {
        let store = store();
        let mut a = Project::new("a");
        a.meta.modified_at = 100;
        let mut b = Project::new("b");
        b.meta.modified_at = 300;
        let mut c = Project::new("c");
        c.meta.modified_at = 200;
        store.save(&a).unwrap();
        store.save(&b).unwrap();
        store.save(&c).unwrap();
        let names: Vec<_> = store
            .list()
            .unwrap()
            .into_iter()
            .map(|p| p.meta.name)
            .collect();
        assert_eq!(names, vec!["b", "c", "a"]);
    }

    #[test]
    fn get_missing_returns_none_and_list_empty_on_fresh_store() {
        let store = store();
        assert!(store.get(ProjectId::new()).unwrap().is_none());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn delete_removes_project() {
        let store = store();
        let project = Project::new("doomed");
        store.save(&project).unwrap();
        store.delete(project.id);
        assert!(store.get(project.id).unwrap().is_none());
        assert!(store.list().unwrap().is_empty());
    }
}
