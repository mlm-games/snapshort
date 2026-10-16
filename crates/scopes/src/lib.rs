//! Scopes: what a frame's pixels say about it.
//!
//! Histogram, waveform and RGB parade computed straight from RGBA8, the same
//! pixels the compositor produced. No decode step, no GPU: an agent can ask
//! for a scope and get numbers back it can reason about.

/// One channel's value distribution over 256 bins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Histogram {
    pub r: [u32; 256],
    pub g: [u32; 256],
    pub b: [u32; 256],
    pub luma: [u32; 256],
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            r: [0; 256],
            g: [0; 256],
            b: [0; 256],
            luma: [0; 256],
        }
    }
}

impl Histogram {
    pub fn of(rgba: &[u8]) -> Self {
        let mut out = Self::default();
        for pixel in rgba.chunks_exact(4) {
            let (r, g, b) = (
                u32::from(pixel[0]),
                u32::from(pixel[1]),
                u32::from(pixel[2]),
            );
            // Rec. 709 luma, rounded.
            let luma = ((r * 2126 + g * 7152 + b * 722 + 5_000) / 10_000) as usize;
            out.r[r as usize] += 1;
            out.g[g as usize] += 1;
            out.b[b as usize] += 1;
            out.luma[luma.min(255)] += 1;
        }
        out
    }

    /// Peak bin count across every channel, for scaling a drawn scope.
    pub fn peak(&self) -> u32 {
        self.r
            .iter()
            .chain(self.g.iter())
            .chain(self.b.iter())
            .chain(self.luma.iter())
            .copied()
            .max()
            .unwrap_or(1)
    }

    pub fn total(&self) -> u32 {
        self.luma.iter().sum()
    }
}

/// A waveform: for each screen column, the brightest value of every row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Waveform {
    pub width: usize,
    pub height: usize,
    /// Row-major, one byte per cell: the peak value at that row and column.
    pub cells: Vec<u8>,
}

impl Waveform {
    /// A `width` x `height` waveform of an RGBA8 image, sampled on a grid.
    pub fn of(
        rgba: &[u8],
        width: usize,
        height: usize,
        out_width: usize,
        out_height: usize,
    ) -> Self {
        let mut cells = vec![0u8; out_width * out_height];
        if width == 0 || height == 0 || out_width == 0 || out_height == 0 {
            return Self {
                width: out_width,
                height: out_height,
                cells,
            };
        }
        for y in 0..height {
            for x in 0..width {
                let pixel = &rgba[(y * width + x) * 4..(y * width + x) * 4 + 3];
                let luma = ((u32::from(pixel[0]) * 2126
                    + u32::from(pixel[1]) * 7152
                    + u32::from(pixel[2]) * 722)
                    / 10_000) as usize;
                let row = out_height - 1 - (luma * out_height / 256).min(out_height - 1);
                let col = x * out_width / width;
                let cell = &mut cells[row * out_width + col];
                *cell = (*cell).max(luma.min(255) as u8);
            }
        }
        Self {
            width: out_width,
            height: out_height,
            cells,
        }
    }

    /// Rows as (top to bottom) strings of `#`-scaled density, for text output.
    pub fn as_rows(&self) -> Vec<String> {
        let mut rows = Vec::with_capacity(self.height);
        for y in 0..self.height {
            let mut row = String::with_capacity(self.width);
            for x in 0..self.width {
                let cell = self.cells[y * self.width + x];
                let level = cell as usize * 9 / 255;
                row.push(match level {
                    0 => ' ',
                    1 => '.',
                    2 => ':',
                    3 => '-',
                    4 => '=',
                    5 => '+',
                    6 => '*',
                    7 => '#',
                    8 => '%',
                    _ => '@',
                });
            }
            rows.push(row);
        }
        rows
    }
}

/// Whole-frame statistics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Levels {
    pub r_min: u8,
    pub r_max: u8,
    pub g_min: u8,
    pub g_max: u8,
    pub b_min: u8,
    pub b_max: u8,
    /// Mean luma, 0-255.
    pub luma_mean: u8,
    /// Fraction of pixels at 0 and at 255, in per mille.
    pub clipped_blacks_permille: u32,
    pub clipped_whites_permille: u32,
}

impl Levels {
    pub fn of(rgba: &[u8]) -> Self {
        if rgba.len() < 4 {
            return Self::default();
        }
        let mut out = Self {
            r_min: 255,
            r_max: 0,
            g_min: 255,
            g_max: 0,
            b_min: 255,
            b_max: 0,
            ..Self::default()
        };
        let mut luma_sum = 0u64;
        let mut blacks = 0u64;
        let mut whites = 0u64;
        let mut count = 0u64;
        for pixel in rgba.chunks_exact(4) {
            let (r, g, b) = (pixel[0], pixel[1], pixel[2]);
            out.r_min = out.r_min.min(r);
            out.r_max = out.r_max.max(r);
            out.g_min = out.g_min.min(g);
            out.g_max = out.g_max.max(g);
            out.b_min = out.b_min.min(b);
            out.b_max = out.b_max.max(b);
            let luma = ((u32::from(r) * 2126 + u32::from(g) * 7152 + u32::from(b) * 722) / 10_000)
                .min(255) as u64;
            luma_sum += luma;
            if luma == 0 {
                blacks += 1;
            }
            if luma >= 255 {
                whites += 1;
            }
            count += 1;
        }
        if count > 0 {
            out.luma_mean = (luma_sum / count) as u8;
            out.clipped_blacks_permille = ((blacks * 1000) / count) as u32;
            out.clipped_whites_permille = ((whites * 1000) / count) as u32;
        }
        out
    }

    /// True when the frame uses the full range it could.
    pub fn is_full_range(&self) -> bool {
        self.r_min == 0 && self.g_min == 0 && self.b_min == 0 && self.r_max >= 250
    }
}

/// A colour temperature estimate from the R/B balance, in kelvin-ish units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Balance {
    pub mean_r: u8,
    pub mean_g: u8,
    pub mean_b: u8,
}

impl Balance {
    pub fn of(rgba: &[u8]) -> Self {
        let (mut r, mut g, mut b, mut count) = (0u64, 0u64, 0u64, 0u64);
        for pixel in rgba.chunks_exact(4) {
            r += u64::from(pixel[0]);
            g += u64::from(pixel[1]);
            b += u64::from(pixel[2]);
            count += 1;
        }
        if count == 0 {
            return Self::default();
        }
        Self {
            mean_r: (r / count) as u8,
            mean_g: (g / count) as u8,
            mean_b: (b / count) as u8,
        }
    }

    /// "cool" when blue leads red, "warm" when red leads blue.
    pub fn cast(&self) -> &'static str {
        match self.mean_r as i32 - self.mean_b as i32 {
            diff if diff > 12 => "warm",
            diff if diff < -12 => "cool",
            _ => "neutral",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(pixels: &[(u8, u8, u8)]) -> Vec<u8> {
        let mut out = Vec::with_capacity(pixels.len() * 4);
        for (r, g, b) in pixels {
            out.extend_from_slice(&[*r, *g, *b, 255]);
        }
        out
    }

    #[test]
    fn a_histogram_counts_every_pixel() {
        let rgba = frame(&[(0, 0, 0), (255, 255, 255), (10, 20, 30)]);
        let histogram = Histogram::of(&rgba);
        assert_eq!(histogram.total(), 3);
        assert_eq!(histogram.luma[0], 1, "black lands in bin 0");
        assert_eq!(histogram.luma[255], 1, "white lands in bin 255");
        assert_eq!(histogram.r[10], 1);
        assert_eq!(histogram.g[20], 1);
        assert_eq!(histogram.b[30], 1);
    }

    #[test]
    fn a_waveform_puts_bright_pixels_high() {
        // A 2x1 frame: left black, right white.
        let rgba = frame(&[(0, 0, 0), (255, 255, 255)]);
        let waveform = Waveform::of(&rgba, 2, 1, 8, 8);
        // The right half is bright, so the top row carries it; the left half is
        // dark and stays at the bottom.
        assert!(waveform.cells[4] > 200, "white is high in the waveform");
        assert_eq!(waveform.cells[7 * 8 + 4], 0, "nothing dark in that column");
        let rows = waveform.as_rows();
        assert_eq!(rows.len(), 8);
    }

    #[test]
    fn levels_report_range_and_clipping() {
        let dark = frame(&[(0, 0, 0), (4, 4, 4)]);
        let levels = Levels::of(&dark);
        assert_eq!(levels.r_min, 0);
        assert_eq!(levels.r_max, 4);
        assert_eq!(levels.clipped_blacks_permille, 500);
        assert!(!levels.is_full_range());

        let full = frame(&[(0, 0, 0), (255, 255, 255)]);
        assert!(Levels::of(&full).is_full_range());
        assert_eq!(Levels::of(&full).clipped_whites_permille, 500);
    }

    #[test]
    fn balance_names_the_colour_cast() {
        assert_eq!(Balance::of(&frame(&[(200, 100, 50)])).cast(), "warm");
        assert_eq!(Balance::of(&frame(&[(50, 100, 200)])).cast(), "cool");
        assert_eq!(Balance::of(&frame(&[(120, 120, 120)])).cast(), "neutral");
    }

    #[test]
    fn an_empty_frame_is_not_a_panic() {
        assert_eq!(Histogram::of(&[]).total(), 0);
        assert_eq!(Levels::of(&[]), Levels::default());
        assert_eq!(Balance::of(&[]).cast(), "neutral");
        assert!(Waveform::of(&[], 0, 0, 8, 8).cells.iter().all(|c| *c == 0));
    }
}
