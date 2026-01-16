//! Application-level errors

use snapshort_domain::DomainError;
use snapshort_infra_db::DbError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Domain error: {0}")]
    Domain(#[from] DomainError),

    #[error("Database error: {0}")]
    Database(#[from] DbError),

    #[error("Asset not found: {0}")]
    AssetNotFound(uuid::Uuid),

    #[error("Timeline not found: {0}")]
    TimelineNotFound(uuid::Uuid),

    #[error("Project not found: {0}")]
    ProjectNotFound(uuid::Uuid),

    #[error("Media error: {0}")]
    Media(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Operation cancelled")]
    Cancelled,
}

pub type AppResult<T> = Result<T, AppError>;
