//! Domain-level errors (no infrastructure details)

use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum DomainError {
    #[error("Entity not found: {entity_type} with id {id}")]
    NotFound { entity_type: &'static str, id: Uuid },

    #[error("Invalid frame range: start {start} >= end {end}")]
    InvalidFrameRange { start: i64, end: i64 },

    #[error("Track index {index} out of bounds (max: {max})")]
    TrackOutOfBounds { index: usize, max: usize },

    #[error("Clip overlap detected at frame {frame} on track {track}")]
    ClipOverlap { frame: i64, track: usize },

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("Asset not linked: {0}")]
    AssetNotLinked(Uuid),
}

pub type DomainResult<T> = Result<T, DomainError>;
