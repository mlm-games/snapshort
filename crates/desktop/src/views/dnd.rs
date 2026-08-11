use miniter_domain::{ClipId, Timestamp, TrackId};
use snapshort_usecases::AssetId;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct ClipDragPayload {
    pub clip_id: ClipId,
    pub original_start: Timestamp,
    pub original_track: TrackId,
    /// Pointer distance from the clip's leading edge.
    pub grab_offset_us: i64,
}

#[derive(Clone, Debug)]
pub struct TrimPayload {
    pub clip_id: ClipId,
    pub is_start: bool,
}

#[derive(Clone, Debug)]
pub struct AssetDragPayload {
    pub asset_id: AssetId,
}

pub fn as_drag_payload<T: 'static>(payload: T) -> Rc<dyn std::any::Any> {
    Rc::new(payload)
}
