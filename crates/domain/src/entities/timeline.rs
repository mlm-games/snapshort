//! Timeline aggregate root - orchestrates clips and tracks

use crate::{Clip, ClipId, DomainError, DomainResult, Fps, Frame, FrameRange, Resolution};
use im::Vector;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimelineId(pub Uuid);

impl TimelineId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TimelineId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackType {
    Video,
    Audio,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub name: String,
    pub track_type: TrackType,
    pub index: usize,
    pub locked: bool,
    pub visible: bool,
    pub solo: bool,
    pub height: f32,
}

impl Track {
    pub fn video(index: usize) -> Self {
        Self {
            name: format!("V{}", index + 1),
            track_type: TrackType::Video,
            index,
            locked: false,
            visible: true,
            solo: false,
            height: 60.0,
        }
    }

    pub fn audio(index: usize) -> Self {
        Self {
            name: format!("A{}", index + 1),
            track_type: TrackType::Audio,
            index,
            locked: false,
            visible: true,
            solo: false,
            height: 40.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineSettings {
    pub fps: Fps,
    pub resolution: Resolution,
    pub sample_rate: u32,
    pub audio_channels: u8,
}

impl Default for TimelineSettings {
    fn default() -> Self {
        Self {
            fps: Fps::F24,
            resolution: Resolution::HD,
            sample_rate: 48000,
            audio_channels: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub id: TimelineId,
    pub name: String,
    pub settings: TimelineSettings,
    pub video_tracks: Vector<Track>,
    pub audio_tracks: Vector<Track>,
    pub clips: Vector<Clip>,
    pub playhead: Frame,
    pub work_area: Option<FrameRange>,
}

impl Timeline {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: TimelineId::new(),
            name: name.into(),
            settings: TimelineSettings::default(),
            video_tracks: Vector::unit(Track::video(0)),
            audio_tracks: Vector::unit(Track::audio(0)),
            clips: Vector::new(),
            playhead: Frame::ZERO,
            work_area: None,
        }
    }

    pub fn with_settings(mut self, settings: TimelineSettings) -> Self {
        self.settings = settings;
        self
    }

    pub fn duration(&self) -> Frame {
        self.clips
            .iter()
            .map(|c| c.timeline_end())
            .max()
            .unwrap_or(Frame::ZERO)
    }

    pub fn all_tracks(&self) -> impl Iterator<Item = &Track> {
        self.video_tracks.iter().chain(self.audio_tracks.iter())
    }

    pub fn get_clip(&self, id: ClipId) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == id)
    }

    pub fn clips_on_track(&self, track_index: usize) -> impl Iterator<Item = &Clip> {
        self.clips
            .iter()
            .filter(move |c| c.track_index == track_index)
    }

    pub fn clips_at_frame(&self, frame: Frame) -> impl Iterator<Item = &Clip> {
        self.clips
            .iter()
            .filter(move |c| c.timeline_range().contains(frame))
    }

    pub fn add_video_track(mut self) -> Self {
        let index = self.video_tracks.len();
        self.video_tracks.push_back(Track::video(index));
        self
    }

    pub fn add_audio_track(mut self) -> Self {
        let index = self.audio_tracks.len();
        self.audio_tracks.push_back(Track::audio(index));
        self
    }

    pub fn insert_clip(mut self, clip: Clip) -> DomainResult<Self> {
        let max_track = self.video_tracks.len() + self.audio_tracks.len();
        if clip.track_index >= max_track {
            return Err(DomainError::TrackOutOfBounds {
                index: clip.track_index,
                max: max_track.saturating_sub(1),
            });
        }

        for existing in self.clips.iter() {
            if existing.id != clip.id && clip.overlaps(existing) {
                return Err(DomainError::ClipOverlap {
                    frame: clip.timeline_start.0,
                    track: clip.track_index,
                });
            }
        }

        self.clips.push_back(clip);
        Ok(self)
    }

    pub fn remove_clip(mut self, id: ClipId) -> DomainResult<Self> {
        let initial_len = self.clips.len();
        self.clips = self.clips.into_iter().filter(|c| c.id != id).collect();

        if self.clips.len() == initial_len {
            return Err(DomainError::NotFound {
                entity_type: "Clip",
                id: id.0,
            });
        }

        Ok(self)
    }

    pub fn update_clip<F>(mut self, id: ClipId, f: F) -> DomainResult<Self>
    where
        F: FnOnce(Clip) -> DomainResult<Clip>,
    {
        let idx = self
            .clips
            .iter()
            .position(|c| c.id == id)
            .ok_or(DomainError::NotFound {
                entity_type: "Clip",
                id: id.0,
            })?;

        let clip = self.clips[idx].clone();
        let updated = f(clip)?;

        for (i, existing) in self.clips.iter().enumerate() {
            if i != idx && updated.overlaps(existing) {
                return Err(DomainError::ClipOverlap {
                    frame: updated.timeline_start.0,
                    track: updated.track_index,
                });
            }
        }

        self.clips = self.clips.update(idx, updated);
        Ok(self)
    }

    pub fn seek(mut self, frame: Frame) -> Self {
        self.playhead = frame;
        self
    }

    pub fn set_work_area(mut self, range: Option<FrameRange>) -> Self {
        self.work_area = range;
        self
    }

    pub fn ripple_delete(self, id: ClipId) -> DomainResult<Self> {
        let clip = self
            .get_clip(id)
            .ok_or(DomainError::NotFound {
                entity_type: "Clip",
                id: id.0,
            })?
            .clone();

        let track = clip.track_index;
        let duration = clip.effective_duration();
        let end_frame = clip.timeline_end();

        let mut new_timeline = self.remove_clip(id)?;

        new_timeline.clips = new_timeline
            .clips
            .into_iter()
            .map(|mut c| {
                if c.track_index == track && c.timeline_start.0 >= end_frame.0 {
                    c.timeline_start = Frame(c.timeline_start.0 - duration);
                }
                c
            })
            .collect();

        Ok(new_timeline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssetId, ClipType};

    fn test_timeline_with_clips() -> Timeline {
        let timeline = Timeline::new("Test");
        let clip1 = Clip::from_asset(
            AssetId::new(),
            ClipType::Video,
            FrameRange::new_unchecked(0, 100),
            Frame(0),
            0,
        );
        let clip2 = Clip::from_asset(
            AssetId::new(),
            ClipType::Video,
            FrameRange::new_unchecked(0, 50),
            Frame(100),
            0,
        );

        timeline
            .insert_clip(clip1)
            .unwrap()
            .insert_clip(clip2)
            .unwrap()
    }

    #[test]
    fn test_timeline_duration() {
        let timeline = test_timeline_with_clips();
        assert_eq!(timeline.duration().0, 150);
    }

    #[test]
    fn test_overlap_detection() {
        let timeline = Timeline::new("Test");
        let clip1 = Clip::from_asset(
            AssetId::new(),
            ClipType::Video,
            FrameRange::new_unchecked(0, 100),
            Frame(0),
            0,
        );

        let timeline = timeline.insert_clip(clip1).unwrap();

        let overlapping = Clip::from_asset(
            AssetId::new(),
            ClipType::Video,
            FrameRange::new_unchecked(0, 50),
            Frame(50),
            0,
        );

        let result = timeline.insert_clip(overlapping);
        assert!(matches!(result, Err(DomainError::ClipOverlap { .. })));
    }

    #[test]
    fn test_immutability() {
        let timeline1 = Timeline::new("Test");
        let clip = Clip::from_asset(
            AssetId::new(),
            ClipType::Video,
            FrameRange::new_unchecked(0, 100),
            Frame(0),
            0,
        );

        let timeline2 = timeline1.clone().insert_clip(clip).unwrap();

        assert_eq!(timeline1.clips.len(), 0);
        assert_eq!(timeline2.clips.len(), 1);
    }
}
