//! Snapshort Domain Layer
//! Pure business entities and value objects. No I/O, no frameworks.

pub mod entities;
pub mod errors;
pub mod events;
pub mod jobs;
pub mod value_objects;

// Re-export common types at crate root
pub use entities::*;
pub use errors::{DomainError, DomainResult};
pub use events::DomainEvent;
pub use jobs::*;
pub use value_objects::{Fps, Frame, FrameRange, Resolution, Timecode};

/// Prelude for common imports (use in other crates)
pub mod prelude {
    pub use crate::{
        // entities
        Asset,
        AssetId,
        AssetMarker,
        AssetStatus,
        AssetType,
        AudioStream,
        Clip,
        ClipEffects,
        ClipId,
        ClipType,
        CodecInfo,
        // errors
        DomainError,
        // events
        DomainEvent,
        DomainResult,
        // value objects
        Fps,
        Frame,
        FrameRange,
        // jobs
        JobId,
        JobKind,
        JobStatus,
        Project,
        ProjectId,
        ProjectSettings,
        ProxyInfo,
        Resolution,
        Timecode,
        Timeline,
        TimelineId,
        TimelineSettings,
        Track,
        TrackRef,
        TrackType,
        VideoStream,
    };
}
