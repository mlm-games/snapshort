//! Exact media time for captions.
//!
//! Every time is an integer number of microseconds; frame rates are exact rationals, so
//! microseconds, frames and SMPTE timecode convert without drift. Every conversion is total: a
//! zero or negative rate gives zero instead of dividing by zero.

pub const TICKS_PER_SECOND: i64 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameRate {
    pub num: i64,
    pub den: i64,
}

impl Default for FrameRate {
    fn default() -> Self {
        FrameRate::FPS_23_976
    }
}

impl FrameRate {
    pub const FPS_23_976: FrameRate = FrameRate {
        num: 24000,
        den: 1001,
    };
    pub const FPS_24: FrameRate = FrameRate { num: 24, den: 1 };
    pub const FPS_25: FrameRate = FrameRate { num: 25, den: 1 };
    pub const FPS_29_97: FrameRate = FrameRate {
        num: 30000,
        den: 1001,
    };
    pub const FPS_30: FrameRate = FrameRate { num: 30, den: 1 };
    pub const FPS_50: FrameRate = FrameRate { num: 50, den: 1 };
    pub const FPS_59_94: FrameRate = FrameRate {
        num: 60000,
        den: 1001,
    };
    pub const FPS_60: FrameRate = FrameRate { num: 60, den: 1 };

    pub fn new(num: i64, den: i64) -> Self {
        let g = gcd(num.abs(), den.abs()).max(1);
        FrameRate {
            num: num / g,
            den: den / g,
        }
    }

    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    pub fn sane(self) -> FrameRate {
        if self.num > 0 && self.den > 0 {
            self
        } else {
            FrameRate::default()
        }
    }

    pub fn frame_duration(self) -> i64 {
        let r = self.sane();
        ((TICKS_PER_SECOND as i128 * r.den as i128) / r.num as i128) as i64
    }

    pub fn frame_at(self, t: i64) -> i64 {
        let r = self.sane();
        (t as i128 * r.num as i128).div_euclid(TICKS_PER_SECOND as i128 * r.den as i128) as i64
    }

    pub fn tick_of(self, f: i64) -> i64 {
        let r = self.sane();
        let n = f as i128 * TICKS_PER_SECOND as i128 * r.den as i128;
        clamp_i64(-(-n).div_euclid(r.num as i128))
    }

    pub fn snap(self, t: i64) -> i64 {
        self.tick_of(self.frame_at(t))
    }

    pub fn snap_nearest(self, t: i64) -> i64 {
        let a = self.snap(t);
        let b = self.tick_of(self.frame_at(t) + 1);
        if (t - a) <= (b - t) { a } else { b }
    }

    pub fn timecode_base(self) -> i64 {
        let r = self.sane();
        ((r.num + r.den - 1) / r.den).max(1)
    }

    pub fn is_ntsc(self) -> bool {
        self.den == 1001
    }

    pub fn supports_drop_frame(self) -> bool {
        self.is_ntsc() && self.timecode_base() % 30 == 0
    }
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn df_params(rate: FrameRate) -> (i64, i64) {
    let base = rate.timecode_base();
    (base / 15, base)
}

pub fn frames_to_fields(
    frame: i64,
    rate: FrameRate,
    drop_frame: bool,
) -> (bool, i64, i64, i64, i64) {
    let neg = frame < 0;
    let mut n = frame.saturating_abs();
    let (drop, base) = df_params(rate);
    if drop_frame && rate.supports_drop_frame() {
        let per_min = base * 60 - drop;
        let per_10 = per_min * 10 + drop;
        let d = n / per_10;
        let m = n % per_10;
        n = n.saturating_add(drop * 9 * d);
        if m > drop {
            n = n.saturating_add(drop * ((m - drop) / per_min));
        }
    }
    let f = n % base;
    let s = (n / base) % 60;
    let mi = (n / (base * 60)) % 60;
    let h = n / (base * 3600);
    (neg, h, mi, s, f)
}

pub fn fields_to_frames(h: i64, m: i64, s: i64, f: i64, rate: FrameRate, drop_frame: bool) -> i64 {
    let (drop, base) = df_params(rate);
    let mut n = h
        .saturating_mul(3600)
        .saturating_add(m.saturating_mul(60))
        .saturating_add(s)
        .saturating_mul(base)
        .saturating_add(f);
    if drop_frame && rate.supports_drop_frame() {
        let total_min = h.saturating_mul(60).saturating_add(m);
        n = n.saturating_sub(drop.saturating_mul(total_min - total_min / 10));
    }
    n
}

fn clamp_i64(value: i128) -> i64 {
    value.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}
