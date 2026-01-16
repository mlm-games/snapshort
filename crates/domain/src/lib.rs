//! Snapshort Domain Layer
//!
//! Pure business entities and value objects. No I/O, no frameworks.
//! This crate defines the core concepts of video editing.

pub mod entities;
pub mod errors;
pub mod events;
pub mod value_objects;

pub use entities::*;
pub use errors::*;
pub use events::*;
pub use value_objects::*;

/// Prelude for common imports
pub mod prelude {
    pub use crate::{
        // Entities
        Asset,
        AssetId,
        AssetStatus,
        AssetType,
        AudioStream,
        Clip,
        ClipEffects,
        ClipId,
        ClipType,
        CodecInfo,
        // Errors
        DomainError,
        // Events
        DomainEvent,

        DomainResult,
        Fps,
        // Value Objects
        Frame,
        FrameRange,
        MediaInfo,
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
        TrackType,
        VideoStream,
    };
}
