use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("Database connection failed: {0}")]
    Connection(#[from] sqlx::Error),

    #[error("Migration failed: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("Entity not found: {entity_type} with id {id}")]
    NotFound { entity_type: &'static str, id: Uuid },

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Constraint violation: {0}")]
    Constraint(String),

    #[error("Transaction failed: {0}")]
    Transaction(String),
}

pub type DbResult<T> = Result<T, DbError>;
