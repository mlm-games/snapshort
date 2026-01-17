//! Immutable value objects - equality by value, not identity

use crate::DomainError;
use derive_more::{Add, Display, From, Into, Sub};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Frame rate (frames per second)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Fps {
    pub num: u32,
    pub den: u32,
}

impl Fps {
    pub const F24: Self = Self { num: 24, den: 1 };
    pub const F30: Self = Self { num: 30, den: 1 };
    pub const F60: Self = Self { num: 60, den: 1 };
    pub const F23976: Self = Self {
        num: 24000,
        den: 1001,
    };

    pub fn new(num: u32, den: u32) -> Self {
        Self {
            num,
            den: den.max(1),
        }
    }

    pub fn as_f64(&self) -> f64 {
        (self.num as f64) / (self.den as f64)
    }

    pub fn frame_duration(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.as_f64())
    }

    pub fn frames_to_duration(&self, frames: i64) -> Duration {
        Duration::from_secs_f64(frames as f64 / self.as_f64())
    }

    pub fn duration_to_frames(&self, duration: Duration) -> i64 {
        (duration.as_secs_f64() * self.as_f64()).round() as i64
    }
}

impl Default for Fps {
    fn default() -> Self {
        Self::F24
    }
}

/// Represents a frame position in the timeline
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    From,
    Into,
    Add,
    Sub,
    Display,
    Ord,
    PartialOrd,
)]
pub struct Frame(pub i64);

impl Frame {
    pub const ZERO: Self = Self(0);

    pub fn new(value: i64) -> Self {
        Self(value)
    }

    pub fn to_timecode(&self, fps: Fps) -> Timecode {
        Timecode::from_frame(*self, fps)
    }

    pub fn to_duration(&self, fps: Fps) -> Duration {
        fps.frames_to_duration(self.0)
    }
}

/// Inclusive frame range [start, end)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FrameRange {
    pub start: Frame,
    pub end: Frame,
}

impl FrameRange {
    pub fn new(start: Frame, end: Frame) -> Result<Self, DomainError> {
        if end.0 < start.0 {
            return Err(DomainError::InvalidFrameRange {
                start: start.0,
                end: end.0,
            });
        }
        Ok(Self { start, end })
    }

    /// Unsafe constructor - use only when you know values are valid
    pub const fn new_unchecked(start: i64, end: i64) -> Self {
        Self {
            start: Frame(start),
            end: Frame(end),
        }
    }

    pub fn duration(&self) -> i64 {
        self.end.0 - self.start.0
    }

    pub fn contains(&self, frame: Frame) -> bool {
        frame.0 >= self.start.0 && frame.0 < self.end.0
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        self.start.0 < other.end.0 && other.start.0 < self.end.0
    }

    pub fn offset(&self, delta: i64) -> Self {
        Self {
            start: Frame(self.start.0 + delta),
            end: Frame(self.end.0 + delta),
        }
    }

    pub fn trim_start(&self, new_start: Frame) -> Result<Self, DomainError> {
        FrameRange::new(new_start, self.end)
    }

    pub fn trim_end(&self, new_end: Frame) -> Result<Self, DomainError> {
        FrameRange::new(self.start, new_end)
    }
}

/// SMPTE Timecode representation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timecode {
    pub hours: u64,
    pub minutes: u64,
    pub seconds: u64,
    pub frames: u64,
    pub negative: bool,
}

impl Timecode {
    pub fn from_frame(frame: Frame, fps: Fps) -> Self {
        let total_frames = frame.0.unsigned_abs();
        let fps_rounded = fps.as_f64().round() as u64;

        let frames = total_frames % fps_rounded;
        let total_seconds = total_frames / fps_rounded;

        let seconds = total_seconds % 60;
        let total_minutes = total_seconds / 60;

        let minutes = total_minutes % 60;
        let hours = total_minutes / 60;

        Self {
            hours,
            minutes,
            seconds,
            frames,
            negative: frame.0 < 0,
        }
    }

    pub fn to_string_smpte(&self) -> String {
        let sign = if self.negative { "-" } else { "" };
        format!(
            "{sign}{:02}:{:02}:{:02}:{:02}",
            self.hours, self.minutes, self.seconds, self.frames
        )
    }
}

impl std::fmt::Display for Timecode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string_smpte())
    }
}

/// Resolution (width x height)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl Resolution {
    pub const HD: Self = Self {
        width: 1920,
        height: 1080,
    };
    pub const UHD: Self = Self {
        width: 3840,
        height: 2160,
    };

    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub fn aspect_ratio(&self) -> f64 {
        self.width as f64 / self.height as f64
    }

    pub fn pixel_count(&self) -> u64 {
        (self.width as u64) * (self.height as u64)
    }

    pub fn fit_within(&self, max_width: u32, max_height: u32) -> Self {
        let scale =
            (max_width as f64 / self.width as f64).min(max_height as f64 / self.height as f64);

        Self {
            width: (self.width as f64 * scale).round() as u32,
            height: (self.height as f64 * scale).round() as u32,
        }
    }
}

impl std::fmt::Display for Resolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fps_conversion() {
        let fps = Fps::new(24, 1);
        assert!((fps.as_f64() - 24.0).abs() < 0.001);
        let fps_ntsc = Fps::new(24000, 1001);
        assert!((fps_ntsc.as_f64() - 23.976).abs() < 0.001);
    }

    #[test]
    fn test_frame_range() {
        let range = FrameRange::new(Frame(0), Frame(100)).unwrap();
        assert_eq!(range.duration(), 100);
        assert!(range.contains(Frame(50)));
        assert!(!range.contains(Frame(100)));
    }

    #[test]
    fn test_frame_range_overlap() {
        let a = FrameRange::new(Frame(0), Frame(100)).unwrap();
        let b = FrameRange::new(Frame(50), Frame(150)).unwrap();
        let c = FrameRange::new(Frame(100), Frame(200)).unwrap();
        assert!(a.overlaps(&b));
        assert!(!a.overlaps(&c));
    }

    #[test]
    fn test_timecode() {
        let tc = Timecode::from_frame(Frame(86400), Fps::F24);
        assert_eq!(tc.hours, 1);
        assert_eq!(tc.minutes, 0);
        assert_eq!(tc.seconds, 0);
        assert_eq!(tc.frames, 0);
    }

    #[test]
    fn test_resolution_fit() {
        let uhd = Resolution::UHD;
        let fitted = uhd.fit_within(1920, 1080);
        assert_eq!(fitted, Resolution::HD);
    }
}
