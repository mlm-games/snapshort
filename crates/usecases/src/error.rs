//! Application-level errors

use snapshort_domain::DomainError;
use snapshort_infra_db::DbError;
use thiserror::Error;
use uuid::Uuid;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Domain error: {0}")]
    Domain(#[from] DomainError),

    #[error("Database error: {0}")]
    Db(#[from] DbError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Project not found: {0}")]
    ProjectNotFound(Uuid),

    #[error("Timeline not found: {0}")]
    TimelineNotFound(Uuid),

    #[error("Asset not found: {0}")]
    AssetNotFound(Uuid),

    #[error("External tool not found in PATH: {tool}")]
    ExternalToolMissing { tool: String },

    #[error("External tool failed: {tool}: {message}")]
    ExternalToolFailed { tool: String, message: String },

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("{0}")]
    Other(String),
}
