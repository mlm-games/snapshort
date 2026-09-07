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

    fn named(name: &str, modified_at: i64) -> Project {
        let mut project = Project::new(name);
        project.meta.modified_at = modified_at;
        project
    }

    #[test]
    fn library_roundtrip_orders_upserts_and_misses() {
        let store = store();
        assert!(store.list().unwrap().is_empty());

        store.save(&named("a", 100)).unwrap();
        store.save(&named("b", 300)).unwrap();
        store.save(&named("c", 200)).unwrap();
        let names: Vec<_> = store
            .list()
            .unwrap()
            .into_iter()
            .map(|p| p.meta.name)
            .collect();
        assert_eq!(names, vec!["b", "c", "a"]);

        // Re-saving overwrites in place (upsert): the old sqlite backend
        // failed here with a primary-key conflict, and the entry stays single.
        let mut b2 = named("b2", 400);
        let first_b = store.list().unwrap()[0].clone();
        b2.id = first_b.id;
        store.save(&b2).unwrap();
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].meta.name, "b2");

        assert!(store.get(ProjectId::new()).unwrap().is_none());
    }

    #[test]
    fn project_deletion_removes_all_traces() {
        let store = store();
        let project = Project::new("doomed");
        store.save(&project).unwrap();
        store.delete(project.id);
        assert!(store.get(project.id).unwrap().is_none());
        assert!(store.list().unwrap().is_empty());
    }
}
