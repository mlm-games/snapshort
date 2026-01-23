//! Shared drag-and-drop payloads for editor views.

use snapshort_domain::{AssetId, ClipId, Frame, TrackRef};
use std::rc::Rc;

/// Payload for clip drag operations.
#[derive(Clone, Debug)]
pub struct ClipDragPayload {
    pub clip_id: ClipId,
    pub original_start: Frame,
    pub original_track: TrackRef,
}

/// Payload for trim handle drag operations.
#[derive(Clone, Debug)]
pub struct TrimPayload {
    pub clip_id: ClipId,
    pub is_start: bool,
    pub original_frame: Frame,
}

/// Payload for dragging assets into the timeline.
#[derive(Clone, Debug)]
pub struct AssetDragPayload {
    pub asset_id: AssetId,
}

/// Helper to wrap a payload in a Repose drag payload.
pub fn as_drag_payload<T: 'static>(payload: T) -> Rc<dyn std::any::Any> {
    Rc::new(payload)
}
