//! Interchange: read and write the formats other tools speak.
//!
//! * FCPXML and CMX3600 EDL export, so a cut moves to Premiere, Resolve or
//!   Final Cut without a re-edit.
//! * SRT cues out of timed text clips, and SRT files back in as a subtitle
//!   track.
//!
//! Everything takes the model's own types; the engine turns them into files.

pub mod edl;
pub mod fcpxml;
pub mod srt;

pub use edl::export_edl;
pub use fcpxml::export_fcpxml;
pub use srt::{Cue, clips_from_cues, cue_to_srt, export_srt, parse_srt};

#[derive(Debug, thiserror::Error)]
pub enum InterchangeError {
    #[error("no timeline is open")]
    NoTimeline,
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

/// A frame rate as a rational, so 24000/1001 stays exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    pub num: u32,
    pub den: u32,
}

impl Rate {
    pub fn new(num: u32, den: u32) -> Self {
        Self {
            num: num.max(1),
            den: den.max(1),
        }
    }

    /// From a float frame rate, keeping four digits of precision (23.976).
    pub fn from_f64(fps: f64) -> Self {
        if !fps.is_finite() || fps <= 0.0 {
            return Self::new(24, 1);
        }
        let den = 1001;
        let num = (fps * den as f64).round() as u32;
        let g = gcd(num, den);
        Self::new(num / g, den / g)
    }

    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Reduce to lowest terms.
    pub fn reduced(self) -> Self {
        let g = gcd(self.num, self.den);
        Self::new(self.num / g, self.den / g)
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Where a clip sits on its track, in frames.
pub struct ClipPlacement {
    pub track_index: usize,
    pub start_frame: i64,
    pub end_frame: i64,
    pub source_start_frame: i64,
    pub source_end_frame: i64,
}
