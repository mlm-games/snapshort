use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("Storage IO error: {0}")]
    Io(String),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Entity not found: {entity_type} with id {id}")]
    NotFound { entity_type: &'static str, id: Uuid },
    #[error("Constraint violation: {0}")]
    Constraint(String),
}

pub type StoreResult<T> = Result<T, StoreError>;

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}
