//! Clip entity - a reference to an asset placed on the timeline
use crate::{AssetId, DomainError, DomainResult, Frame, FrameRange, TrackRef};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClipId(pub Uuid);

impl ClipId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ClipId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClipType {
    Video,
    Audio,
    Title,
    Generator,
    Adjustment,
    Gap,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipEffects {
    pub opacity: f32,
    pub volume: f32,
    pub speed: f32,
    pub reverse: bool,
    pub position: (f32, f32),
    pub scale: (f32, f32),
    pub rotation: f32,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
}

impl Default for ClipEffects {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            volume: 1.0,
            speed: 1.0,
            reverse: false,
            position: (0.0, 0.0),
            scale: (1.0, 1.0),
            rotation: 0.0,
            flip_horizontal: false,
            flip_vertical: false,
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
        }
    }
}

impl ClipEffects {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn adjusted_duration(&self, original_frames: i64) -> i64 {
        (original_frames as f32 / self.speed).round() as i64
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: ClipId,
    pub clip_type: ClipType,
    pub asset_id: Option<AssetId>,
    pub timeline_start: Frame,
    pub track: TrackRef,
    pub source_range: FrameRange,
    pub effects: ClipEffects,
    pub name: Option<String>,
    pub color: Option<String>,
    pub enabled: bool,
    pub locked: bool,
}

impl Clip {
    pub fn from_asset(
        asset_id: AssetId,
        clip_type: ClipType,
        source_range: FrameRange,
        timeline_start: Frame,
        track: TrackRef,
    ) -> Self {
        Self {
            id: ClipId::new(),
            clip_type,
            asset_id: Some(asset_id),
            timeline_start,
            track,
            source_range,
            effects: ClipEffects::new(),
            name: None,
            color: None,
            enabled: true,
            locked: false,
        }
    }

    pub fn gap(timeline_start: Frame, duration: i64, track: TrackRef) -> DomainResult<Self> {
        Ok(Self {
            id: ClipId::new(),
            clip_type: ClipType::Gap,
            asset_id: None,
            timeline_start,
            track,
            source_range: FrameRange::new(Frame(0), Frame(duration))?,
            effects: ClipEffects::new(),
            name: Some("Gap".into()),
            color: None,
            enabled: true,
            locked: false,
        })
    }

    pub fn timeline_range(&self) -> FrameRange {
        let duration = self.effective_duration();
        FrameRange::new_unchecked(self.timeline_start.0, self.timeline_start.0 + duration)
    }

    pub fn effective_duration(&self) -> i64 {
        self.effects.adjusted_duration(self.source_range.duration())
    }

    pub fn timeline_end(&self) -> Frame {
        Frame(self.timeline_start.0 + self.effective_duration())
    }

    pub fn overlaps(&self, other: &Clip) -> bool {
        self.track == other.track && self.timeline_range().overlaps(&other.timeline_range())
    }

    pub fn move_to(&mut self, new_start: Frame, new_track: TrackRef) {
        self.timeline_start = new_start;
        self.track = new_track;
    }

    pub fn trim_start(&mut self, new_timeline_start: Frame) -> DomainResult<()> {
        let delta = new_timeline_start.0 - self.timeline_start.0;
        if delta <= 0 {
            return Err(DomainError::InvalidOperation(
                "Cannot extend clip past original start".into(),
            ));
        }
        let new_source_start = Frame(self.source_range.start.0 + delta);
        self.source_range = FrameRange::new(new_source_start, self.source_range.end)?;
        self.timeline_start = new_timeline_start;
        Ok(())
    }

    pub fn trim_end(&mut self, new_timeline_end: Frame) -> DomainResult<()> {
        let new_duration = new_timeline_end.0 - self.timeline_start.0;
        if new_duration <= 0 {
            return Err(DomainError::InvalidOperation(
                "Clip duration must be positive".into(),
            ));
        }
        let new_source_end = Frame(self.source_range.start.0 + new_duration);
        self.source_range = FrameRange::new(self.source_range.start, new_source_end)?;
        Ok(())
    }

    pub fn split_at(&mut self, split_frame: Frame) -> DomainResult<Clip> {
        let range = self.timeline_range();
        if !range.contains(split_frame) {
            return Err(DomainError::InvalidOperation(format!(
                "Split frame {} not within clip range {:?}",
                split_frame.0, range
            )));
        }
        let offset_in_clip = split_frame.0 - self.timeline_start.0;
        let source_split = Frame(self.source_range.start.0 + offset_in_clip);

        let mut right = self.clone();
        right.id = ClipId::new();
        right.timeline_start = split_frame;
        right.source_range = FrameRange::new(source_split, self.source_range.end)?;

        self.source_range = FrameRange::new(self.source_range.start, source_split)?;
        Ok(right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssetId, TrackRef};

    fn test_clip() -> Clip {
        Clip::from_asset(
            AssetId::new(),
            ClipType::Video,
            FrameRange::new_unchecked(0, 100),
            Frame(50),
            TrackRef::video(0),
        )
    }

    #[test]
    fn test_timeline_range() {
        let clip = test_clip();
        let range = clip.timeline_range();
        assert_eq!(range.start.0, 50);
        assert_eq!(range.end.0, 150);
    }

    #[test]
    fn test_clip_overlap() {
        let clip1 = test_clip();
        let mut clip2 = test_clip();
        clip2.timeline_start = Frame(100);
        assert!(clip1.overlaps(&clip2));
        clip2.timeline_start = Frame(150);
        assert!(!clip1.overlaps(&clip2));
    }

    #[test]
    fn test_split() {
        let mut clip = test_clip();
        let right = clip.split_at(Frame(100)).unwrap();
        assert_eq!(clip.timeline_range().duration(), 50);
        assert_eq!(right.timeline_range().duration(), 50);
        assert_eq!(right.timeline_start.0, 100);
    }

    #[test]
    fn test_speed_effect() {
        let mut clip = test_clip();
        clip.effects.speed = 2.0;
        assert_eq!(clip.effective_duration(), 50);
    }

    #[test]
    fn test_color_defaults() {
        let clip = test_clip();
        assert_eq!(clip.effects.brightness, 0.0);
        assert_eq!(clip.effects.contrast, 0.0);
        assert_eq!(clip.effects.saturation, 0.0);
    }
}
