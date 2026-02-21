//! Shared UI Components for Repose

use repose_canvas::{Canvas, DrawScope};
use repose_core::{Color, Modifier, Rect, Vec2, View};
use repose_ui::{Button, Text, TextStyle};

pub mod colors {
    use repose_core::Color;

    // Background Colors
    pub const BG_DARK: Color = Color(24, 24, 28, 255);
    pub const BG_MEDIUM: Color = Color(32, 32, 38, 255);
    pub const BG_LIGHT: Color = Color(44, 44, 52, 255);
    pub const BG_PANEL: Color = Color(28, 28, 34, 255);
    pub const BG_HEADER: Color = Color(36, 36, 42, 255);
    pub const BG_TRACK: Color = Color(26, 26, 32, 255);
    pub const BG_SELECTED: Color = Color(88, 166, 255, 60);
    pub const BG_HOVER: Color = Color(88, 166, 255, 30);
    pub const BG_ACTIVE_TAB: Color = Color(32, 32, 40, 255);

    // Text Colors
    pub const TEXT_PRIMARY: Color = Color(240, 240, 245, 255);
    pub const TEXT_MUTED: Color = Color(140, 140, 150, 255);
    pub const TEXT_DISABLED: Color = Color(80, 80, 90, 255);
    pub const TEXT_HEADER: Color = Color(220, 220, 230, 255);
    pub const TEXT_ACCENT: Color = Color(88, 166, 255, 255);

    // Accent Colors
    pub const ACCENT: Color = Color(25, 25, 112, 255);
    pub const ACCENT_HOVER: Color = Color(45, 45, 142, 255);

    // Border Colors
    pub const BORDER: Color = Color(60, 60, 70, 255);

    // Track Colors
    pub const VIDEO_TRACK: Color = Color(74, 144, 226, 255);
    pub const AUDIO_TRACK: Color = Color(82, 190, 128, 255);

    // Status Colors
    pub const SUCCESS: Color = Color(82, 190, 128, 255);
    pub const WARNING: Color = Color(240, 178, 88, 255);
    pub const ERROR: Color = Color(237, 76, 103, 255);

    // Utility Colors
    pub const TRANSPARENT: Color = Color(0, 0, 0, 0);
}

pub fn primary_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(Text(label).color(colors::TEXT_PRIMARY).size(14.0), on_click).modifier(
        Modifier::new()
            .background(colors::ACCENT)
            .padding(12.0)
            .clip_rounded(6.0),
    )
}

pub fn icon_button(icon: &str, on_click: impl Fn() + 'static) -> View {
    Button(Text(icon).size(18.0).color(colors::TEXT_PRIMARY), on_click).modifier(
        Modifier::new()
            .padding(8.0)
            .clip_rounded(4.0)
            .align_items(repose_core::AlignItems::Center)
            .justify_content(repose_core::JustifyContent::Center),
    )
}

pub fn playhead(playhead_frame: i64, px_per_frame: f32, on_seek: impl Fn(i64) + 'static) -> View {
    let x = playhead_frame as f32 * px_per_frame;
    let line_color = colors::ACCENT;

    Canvas(
        Modifier::new().fill_max_height().width(12.0),
        move |scope: &mut DrawScope| {
            let height = scope.size.height;
            let width = scope.size.width;

            scope.draw_rect_stroke(
                Rect {
                    x: width / 2.0 - 0.5,
                    y: 0.0,
                    w: 1.0,
                    h: height,
                },
                line_color,
                0.0,
                1.0,
            );

            scope.draw_circle(
                Vec2 {
                    x: width / 2.0,
                    y: 6.0,
                },
                5.0,
                line_color,
            );
        },
    )
    .modifier(
        Modifier::new()
            .absolute()
            .offset(Some(x - 6.0), Some(0.0), None, None)
            .z_index(100.0)
            .clickable()
            .on_pointer_down({
                move |event| {
                    let frame = (event.position.x / px_per_frame).round() as i64;
                    on_seek(frame.max(0));
                }
            }),
    )
}

/// Render an audio waveform visualization
///
/// # Arguments
/// * `width` - Total width of the waveform in pixels
/// * `height` - Height of the waveform in pixels
/// * `waveform_data` - Optional peak amplitude data (0.0-1.0). If None, generates placeholder pattern.
/// * `color` - Color of the waveform bars
pub fn audio_waveform(
    width: f32,
    height: f32,
    waveform_data: Option<&[f32]>,
    color: Color,
) -> View {
    // Generate sample count based on width (roughly 1 sample per 3-4 pixels looks good)
    let sample_count = ((width / 3.5).ceil() as usize).max(4).min(256);

    // Use provided data or generate placeholder pattern
    let samples: Vec<f32> = if let Some(data) = waveform_data {
        // Resample the provided data to match our display sample count
        if data.is_empty() {
            vec![0.2; sample_count]
        } else {
            let ratio = data.len() as f32 / sample_count as f32;
            (0..sample_count)
                .map(|i| {
                    let idx = (i as f32 * ratio) as usize;
                    data.get(idx).copied().unwrap_or(0.2)
                })
                .collect()
        }
    } else {
        // Generate a procedural waveform pattern that looks natural
        (0..sample_count)
            .map(|i| {
                let t = i as f32 / sample_count as f32;
                // Combine multiple frequencies for natural-looking audio pattern
                let wave1 = (t * 15.7).sin().abs();
                let wave2 = (t * 31.4 + 0.5).sin().abs() * 0.6;
                let wave3 = (t * 7.85 + 1.0).sin().abs() * 0.3;
                let envelope = 0.3 + 0.7 * (1.0 - (t - 0.5).abs() * 1.5).max(0.0);
                ((wave1 + wave2 + wave3) * envelope * 0.5).clamp(0.1, 1.0)
            })
            .collect()
    };

    Canvas(
        Modifier::new().width(width).height(height),
        move |scope: &mut DrawScope| {
            let h = scope.size.height;
            let w = scope.size.width;
            let bar_width = (w / samples.len() as f32).max(1.0) * 0.7;
            let spacing = w / samples.len() as f32;
            let center_y = h / 2.0;

            for (i, &amplitude) in samples.iter().enumerate() {
                let bar_height = (h * amplitude * 0.9).max(2.0);
                let x = i as f32 * spacing + (spacing - bar_width) / 2.0;
                let y = center_y - bar_height / 2.0;

                scope.draw_rect(
                    Rect {
                        x,
                        y,
                        w: bar_width,
                        h: bar_height,
                    },
                    color,
                    1.0,
                );
            }
        },
    )
}
