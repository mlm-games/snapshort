//! Shared UI Components for Repose

use repose_canvas::{Canvas, DrawScope};
use repose_core::{Modifier, Rect, Vec2, View};
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
    pub const ACCENT: Color = Color(88, 166, 255, 255);
    pub const ACCENT_HOVER: Color = Color(108, 186, 255, 255);

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
    Button(Text(icon).size(18.0).color(colors::TEXT_PRIMARY), on_click)
        .modifier(Modifier::new().padding(8.0).clip_rounded(4.0))
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
