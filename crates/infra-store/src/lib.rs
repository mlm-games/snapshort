//! Crash-safe file persistence on `game-utils` [`Storage`](game_utils::storage::Storage).
//!
//! This crate replaces the old sqlite backend (`snapshort-infra-db`, deleted).
//! Projects and background jobs persist as JSON files under an app data dir,
//! written atomically (temp file + rename, `.bak` rotation, corrupt
//! quarantine) via `SaveStore`. The same model runs on every platform: native
//! `FsStorage` here, OPFS JSON snapshots on web (`backend_wasm`).

pub mod error;
pub mod job_store;
pub mod project_store;

pub use error::{StoreError, StoreResult};
pub use job_store::{JobRecord, JobStatus, JobStore};
pub use project_store::ProjectStore;
