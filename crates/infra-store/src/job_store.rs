//! File-backed background-job queue on [`Storage`](game_utils::storage::Storage).
//!
//! Layout under the store root:
//!
//! ```text
//! <root>/<job-id>.json   # `JobRecord` pretty JSON (upsert per mutation)
//! ```
//!
//! One file per job replaces the old `jobs` table. `list_pending` is a dir
//! scan filtered to `queued`/`running` ordered by `created_at` asc — the file
//! equivalent of `SELECT ... WHERE status IN (...) ORDER BY created_at ASC`.
//! Every mutation goes through `SaveStore` (temp + atomic rename, `.bak`),
//! and `SaveStore` siblings (`temp_*`, `*.bak`, `corrupted_*`) are never
//! mistaken for jobs.

use crate::{StoreError, StoreResult};
use game_utils::save_store::SaveStore;
use game_utils::storage::{FsStorage, Storage};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::Succeeded => "succeeded",
            JobStatus::Failed => "failed",
            JobStatus::Canceled => "canceled",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JobRecord {
    pub id: Uuid,
    pub kind: String,
    pub status: JobStatus,
    pub progress: Option<i32>,
    pub payload_json: String,
    pub result_json: Option<String>,
    pub error: Option<String>,
    /// Unix millis.
    pub created_at: i64,
    /// Unix millis.
    pub updated_at: i64,
}

fn now_millis() -> i64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct JobStore<S: Storage = FsStorage> {
    dir: PathBuf,
    storage: S,
}

impl JobStore<FsStorage> {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::new_with_storage(root, FsStorage)
    }
}

impl<S: Storage> JobStore<S> {
    pub fn new_with_storage(root: impl Into<PathBuf>, storage: S) -> Self {
        let dir = root.into().join("jobs");
        let _ = storage.create_dir_all(&dir);
        Self { dir, storage }
    }

    fn slot(&self, id: Uuid) -> SaveStore<S> {
        SaveStore::new_with_storage(
            self.dir.clone(),
            format!("{id}.json"),
            self.storage.clone(),
        )
    }

    fn write_record(&self, record: &JobRecord) -> StoreResult<()> {
        let json = serde_json::to_string_pretty(record)?;
        self.slot(record.id)
            .write(json.as_bytes())
            .map_err(StoreError::Io)?;
        Ok(())
    }

    fn read_record(&self, id: Uuid) -> StoreResult<Option<JobRecord>> {
        let loaded = self.slot(id).load(&SaveStore::<S>::is_intact_json, &[]);
        let Some(bytes) = loaded.data else {
            return Ok(None);
        };
        let text = std::str::from_utf8(&bytes)
            .map_err(|e| StoreError::Constraint(format!("Invalid UTF-8 in job file: {e}")))?;
        Ok(Some(serde_json::from_str(text)?))
    }

    pub fn create(&self, id: Uuid, kind: &str, payload_json: &str) -> StoreResult<()> {
        let now = now_millis();
        self.write_record(&JobRecord {
            id,
            kind: kind.to_string(),
            status: JobStatus::Queued,
            progress: None,
            payload_json: payload_json.to_string(),
            result_json: None,
            error: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn get(&self, id: Uuid) -> StoreResult<Option<JobRecord>> {
        self.read_record(id)
    }

    /// Jobs still needing work (`queued` or `running`), oldest first.
    pub fn list_pending(&self) -> StoreResult<Vec<JobRecord>> {
        let entries = match self.storage.read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        for path in entries {
            if !is_job_file(&path) {
                continue;
            }
            let Ok(Some(bytes)) = self.storage.read(&path) else {
                continue;
            };
            let Ok(record) = serde_json::from_slice::<JobRecord>(&bytes) else {
                continue;
            };
            if matches!(record.status, JobStatus::Queued | JobStatus::Running) {
                out.push(record);
            }
        }
        out.sort_by_key(|r| r.created_at);
        Ok(out)
    }

    /// Recovery rule: anything `running` becomes `queued` on startup.
    /// Returns the number of jobs recovered.
    pub fn recover_incomplete(&self) -> StoreResult<u64> {
        let pending = self.list_pending()?;
        let mut recovered = 0u64;
        for mut record in pending {
            if record.status == JobStatus::Running {
                record.status = JobStatus::Queued;
                record.updated_at = now_millis();
                self.write_record(&record)?;
                recovered += 1;
            }
        }
        Ok(recovered)
    }

    fn mutate(&self, id: Uuid, f: impl FnOnce(&mut JobRecord)) -> StoreResult<()> {
        let Some(mut record) = self.read_record(id)? else {
            return Err(StoreError::NotFound {
                entity_type: "job",
                id,
            });
        };
        f(&mut record);
        record.updated_at = now_millis();
        self.write_record(&record)
    }

    pub fn set_running(&self, id: Uuid) -> StoreResult<()> {
        self.mutate(id, |r| {
            r.status = JobStatus::Running;
            r.progress = None;
            r.result_json = None;
            r.error = None;
        })
    }

    pub fn set_progress(&self, id: Uuid, progress: u8) -> StoreResult<()> {
        self.mutate(id, |r| {
            r.status = JobStatus::Running;
            r.progress = Some(progress as i32);
        })
    }

    pub fn set_succeeded(&self, id: Uuid, result_json: Option<String>) -> StoreResult<()> {
        self.mutate(id, |r| {
            r.status = JobStatus::Succeeded;
            r.progress = Some(100);
            r.result_json = result_json.clone();
            r.error = None;
        })
    }

    pub fn set_failed(&self, id: Uuid, error: String) -> StoreResult<()> {
        self.mutate(id, |r| {
            r.status = JobStatus::Failed;
            r.error = Some(error.clone());
        })
    }

    pub fn set_canceled(&self, id: Uuid) -> StoreResult<()> {
        self.mutate(id, |r| {
            r.status = JobStatus::Canceled;
        })
    }
}

fn is_job_file(path: &std::path::Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.ends_with(".json") && !name.starts_with("temp_") && !name.starts_with("corrupted_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_utils::storage::MemoryStorage;

    fn store() -> JobStore<MemoryStorage> {
        JobStore::new_with_storage("/test-jobs", MemoryStorage::new())
    }

    #[test]
    fn full_lifecycle() {
        let store = store();
        let id = Uuid::new_v4();
        store.create(id, "analyze_asset", "{}").unwrap();

        let record = store.get(id).unwrap().unwrap();
        assert_eq!(record.status, JobStatus::Queued);
        assert_eq!(record.kind, "analyze_asset");

        store.set_running(id).unwrap();
        store.set_progress(id, 5).unwrap();
        let record = store.get(id).unwrap().unwrap();
        assert_eq!(record.status, JobStatus::Running);
        assert_eq!(record.progress, Some(5));

        store.set_succeeded(id, None).unwrap();
        let record = store.get(id).unwrap().unwrap();
        assert_eq!(record.status, JobStatus::Succeeded);
        assert_eq!(record.progress, Some(100));

        // Finished jobs leave the pending queue.
        assert!(store.list_pending().unwrap().is_empty());
    }

    #[test]
    fn recover_running_to_queued() {
        let store = store();
        let id = Uuid::new_v4();
        store.create(id, "k", "{}").unwrap();
        store.set_running(id).unwrap();

        assert_eq!(store.recover_incomplete().unwrap(), 1);
        let record = store.get(id).unwrap().unwrap();
        assert_eq!(record.status, JobStatus::Queued);

        // Second recovery is a no-op.
        assert_eq!(store.recover_incomplete().unwrap(), 0);
    }

    #[test]
    fn pending_lists_queued_and_running_oldest_first() {
        let store = store();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        store.create(a, "k", "{}").unwrap();
        store.create(b, "k", "{}").unwrap();
        store.create(c, "k", "{}").unwrap();
        // Force distinct timestamps (millis clock may tie on fast machines).
        {
            let mut ra = store.get(a).unwrap().unwrap();
            ra.created_at = 300;
            store.write_record(&ra).unwrap();
            let mut rb = store.get(b).unwrap().unwrap();
            rb.created_at = 100;
            store.write_record(&rb).unwrap();
            let mut rc = store.get(c).unwrap().unwrap();
            rc.created_at = 200;
            store.write_record(&rc).unwrap();
        }
        store.set_succeeded(a, None).unwrap();

        let pending = store.list_pending().unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].id, b);
        assert_eq!(pending[1].id, c);
    }

    #[test]
    fn fail_and_cancel() {
        let store = store();
        let id = Uuid::new_v4();
        store.create(id, "k", "{}").unwrap();
        store.set_failed(id, "boom".to_string()).unwrap();
        let record = store.get(id).unwrap().unwrap();
        assert_eq!(record.status, JobStatus::Failed);
        assert_eq!(record.error.as_deref(), Some("boom"));

        let id2 = Uuid::new_v4();
        store.create(id2, "k", "{}").unwrap();
        store.set_canceled(id2).unwrap();
        assert_eq!(store.get(id2).unwrap().unwrap().status, JobStatus::Canceled);
        assert!(store.list_pending().unwrap().is_empty());
    }

    #[test]
    fn missing_job_is_none_or_not_found() {
        let store = store();
        let id = Uuid::new_v4();
        assert!(store.get(id).unwrap().is_none());
        let err = store.set_running(id).unwrap_err();
        assert!(matches!(err, StoreError::NotFound { .. }));
    }
}
