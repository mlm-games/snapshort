//! Canonical time rules for snapshort.
//!
//! The model (`miniter-domain`) stores time as **integer microseconds**
//! (`Timestamp` / `MediaDuration`, `i64`). This module states the rules every
//! edit, playback, and render path must follow:
//!
//! 1. **Integers cross boundaries.** Microseconds cross function, thread, and
//!    process boundaries. Float seconds exist only at display, probe-metadata,
//!    and sleep-timer edges — never in edit math.
//! 2. **Speed scaling rounds.** Clip `speed` is a float ratio; converting
//!    between timeline and source microseconds must round to nearest
//!    ([`scale_us_round`] / [`unscale_us_round`]), never truncate. Truncation
//!    biases every trim/split by up to 1µs in one direction and accumulates.
//! 3. **Frame stepping carries the remainder.** Advancing a playhead by
//!    `1_000_000 / fps` with integer division drops the fraction every frame
//!    (24fps loses 0.67µs/frame). [`FrameStepper`] accumulates the remainder
//!    so the long-term average is exactly 1µs × 10^6 / fps.
//! 4. **Frame boundaries are rational.** [`frame_index`] / [`frame_start_us`]
//!    compute frame↔micro conversions with integer math (`i128` internally),
//!    so 24000/1001-style rates stay exact.

pub use miniter_domain::time::{scale_us_round, unscale_us_round};

/// Exact per-frame playhead stepper (Bresenham remainder).
///
/// Each [`FrameStepper::next_step_us`] returns the microsecond step for one
/// frame; the long-term average is exactly `1_000_000 / fps`. Changing fps
/// resets the carried remainder so no phantom mega-step follows a rate change.
#[derive(Debug, Clone)]
pub struct FrameStepper {
    fps: i64,
    carry_us: i64,
}

impl FrameStepper {
    pub fn new(fps: i64) -> Self {
        Self {
            fps: fps.max(1),
            carry_us: 0,
        }
    }

    pub fn set_fps(&mut self, fps: i64) {
        self.fps = fps.max(1);
        self.carry_us = 0;
    }

    /// Drop the carried fraction (call on seek/stop so playback resumes on a
    /// whole-frame phase instead of emitting a catch-up step).
    pub fn reset(&mut self) {
        self.carry_us = 0;
    }

    pub fn next_step_us(&mut self) -> i64 {
        // carry_us stays in [0, fps): no overflow for any sane fps.
        self.carry_us += 1_000_000;
        let step = self.carry_us / self.fps;
        self.carry_us %= self.fps;
        step
    }
}

/// 0-based frame index containing timestamp `us` at `num`/`den` fps (floor).
pub fn frame_index(us: i64, num: u32, den: u32) -> i64 {
    let num = num.max(1) as i128;
    let den = den.max(1) as i128;
    ((us.max(0) as i128) * num / (1_000_000 * den)) as i64
}

/// Start timestamp (µs) of frame `index` at `num`/`den` fps.
pub fn frame_start_us(index: i64, num: u32, den: u32) -> i64 {
    let num = num.max(1) as i128;
    let den = den.max(1) as i128;
    ((index.max(0) as i128) * 1_000_000 * den / num) as i64
}

/// Nearest frame-boundary timestamp (ties round up).
pub fn quantize_to_frame_us(us: i64, num: u32, den: u32) -> i64 {
    let idx = frame_index(us, num, den);
    let start = frame_start_us(idx, num, den);
    let next = frame_start_us(idx + 1, num, den);
    if us - start < next - us { start } else { next }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepper_averages_exactly() {
        // 24fps: true step 41666.67µs. 2400 frames must total exactly 100s.
        let mut stepper = FrameStepper::new(24);
        let total: i64 = (0..2400).map(|_| stepper.next_step_us()).sum();
        assert_eq!(total, 100_000_000);
        // Steps dither between the two adjacent integers, nothing else.
        let mut stepper = FrameStepper::new(24);
        let steps: Vec<i64> = (0..48).map(|_| stepper.next_step_us()).collect();
        assert!(steps.iter().all(|&s| s == 41_666 || s == 41_667));
    }

    #[test]
    fn stepper_exact_for_integer_rates() {
        let mut stepper = FrameStepper::new(25);
        assert!(std::iter::repeat_with(|| stepper.next_step_us())
            .take(100)
            .all(|s| s == 40_000));
    }

    #[test]
    fn frame_boundaries_round_correctly() {
        // 24fps: frame 1 truly starts at 41666.67µs, so 41666 is still frame 0.
        assert_eq!(frame_start_us(0, 24, 1), 0);
        assert_eq!(frame_start_us(1, 24, 1), 41_666);
        assert_eq!(frame_index(41_666, 24, 1), 0);
        assert_eq!(frame_index(41_667, 24, 1), 1);
        assert_eq!(quantize_to_frame_us(20_000, 24, 1), 0);
        assert_eq!(quantize_to_frame_us(30_000, 24, 1), 41_666);
        // 24000/1001: frame 1 starts at 41708.33µs.
        assert_eq!(frame_start_us(1, 24_000, 1001), 41_708);
        assert_eq!(frame_index(41_708, 24_000, 1001), 0);
        assert_eq!(frame_index(41_709, 24_000, 1001), 1);
    }
}
