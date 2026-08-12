//! Shared UI Components for Snapshort (Material 3 inspired)

use repose_canvas::{Canvas, DrawScope};
use repose_core::{Color, Modifier, Rect, View};
use repose_material::Symbol;

pub mod colors {
    use repose_core::Color;

    // Surfaces — cool NLE grays (aligned with snapshort_theme)
    pub const BG_DARK: Color = Color(12, 14, 18, 255);
    pub const BG_MEDIUM: Color = Color(22, 26, 34, 255);
    pub const BG_LIGHT: Color = Color(36, 42, 52, 255);
    pub const BG_PANEL: Color = Color(26, 30, 38, 255);
    pub const BG_HEADER: Color = Color(32, 37, 46, 255);
    pub const BG_TRACK: Color = Color(18, 22, 28, 255);
    pub const BG_SELECTED: Color = Color(76, 154, 255, 70);
    pub const BG_HOVER: Color = Color(76, 154, 255, 36);
    pub const BG_ACTIVE_TAB: Color = Color(28, 32, 40, 255);

    pub const TEXT_PRIMARY: Color = Color(232, 234, 237, 255);
    pub const TEXT_MUTED: Color = Color(150, 158, 172, 255);
    pub const TEXT_DISABLED: Color = Color(90, 96, 108, 255);
    pub const TEXT_HEADER: Color = Color(220, 224, 232, 255);
    pub const TEXT_ACCENT: Color = Color(76, 154, 255, 255);

    // WAS Color(25,25,112) — invisible on dark. Use real accent.
    pub const ACCENT: Color = Color(76, 154, 255, 255);
    pub const ACCENT_HOVER: Color = Color(110, 176, 255, 255);
    pub const ACCENT_CYAN: Color = Color(64, 224, 208, 255);
    pub const PLAYHEAD: Color = Color(255, 72, 72, 255);

    pub const BORDER: Color = Color(48, 54, 66, 255);

    // Clip body fills (solid enough to read at a glance)
    pub const VIDEO_TRACK: Color = Color(58, 120, 200, 255);
    pub const VIDEO_TRACK_DIM: Color = Color(40, 78, 130, 255);
    pub const AUDIO_TRACK: Color = Color(46, 160, 110, 255);
    pub const AUDIO_TRACK_DIM: Color = Color(32, 110, 78, 255);
    pub const TEXT_TRACK: Color = Color(160, 120, 60, 255);
    pub const SUBTITLE_TRACK: Color = Color(140, 100, 180, 255);

    pub const SUCCESS: Color = Color(82, 190, 128, 255);
    pub const WARNING: Color = Color(240, 178, 88, 255);
    pub const ERROR: Color = Color(237, 76, 103, 255);

    pub const MARKER: Color = Color(255, 202, 40, 255);

    pub const TRANSPARENT: Color = Color(0, 0, 0, 0);

    pub fn with_alpha(c: Color, a: u8) -> Color {
        Color(c.0, c.1, c.2, a)
    }
}

/// Material Symbols used across the app (using repose's Symbol type)
pub struct Icons;
impl Icons {
    pub const add: Symbol = Symbol::new("add", '\u{E145}');
    pub const delete: Symbol = Symbol::new("delete", '\u{E872}');
    pub const upload: Symbol = Symbol::new("upload", '\u{E2C6}');
    pub const bolt: Symbol = Symbol::new("bolt", '\u{EA0B}');
    pub const undo: Symbol = Symbol::new("undo", '\u{E166}');
    pub const redo: Symbol = Symbol::new("redo", '\u{E15A}');
    pub const content_cut: Symbol = Symbol::new("content_cut", '\u{E14E}');
    pub const movie: Symbol = Symbol::new("movie", '\u{E02C}');
    pub const music_note: Symbol = Symbol::new("music_note", '\u{E405}');
    pub const image: Symbol = Symbol::new("image", '\u{E3F4}');
    pub const burst_mode: Symbol = Symbol::new("burst_mode", '\u{E43C}');
    pub const info: Symbol = Symbol::new("info", '\u{E88E}');
    pub const warning: Symbol = Symbol::new("warning", '\u{E002}');
    pub const error: Symbol = Symbol::new("error", '\u{E000}');
    pub const check_circle: Symbol = Symbol::new("check_circle", '\u{E86C}');
    pub const play_arrow: Symbol = Symbol::new("play_arrow", '\u{E037}');
    pub const pause: Symbol = Symbol::new("pause", '\u{E034}');
    pub const stop: Symbol = Symbol::new("stop", '\u{E047}');
    pub const skip_previous: Symbol = Symbol::new("skip_previous", '\u{E045}');
    pub const skip_next: Symbol = Symbol::new("skip_next", '\u{E044}');
    pub const fast_rewind: Symbol = Symbol::new("fast_rewind", '\u{E020}');
    pub const fast_forward: Symbol = Symbol::new("fast_forward", '\u{E01F}');
    pub const tune: Symbol = Symbol::new("tune", '\u{E429}');
    pub const blur_on: Symbol = Symbol::new("blur_on", '\u{E3CA}');
    pub const auto_fix: Symbol = Symbol::new("auto_fix", '\u{E648}');
    pub const crop: Symbol = Symbol::new("crop", '\u{E3BE}');
    pub const transform: Symbol = Symbol::new("transform", '\u{E42A}');
    pub const layers: Symbol = Symbol::new("layers", '\u{E53D}');
    pub const equalizer: Symbol = Symbol::new("equalizer", '\u{E01D}');
    pub const volume_up: Symbol = Symbol::new("volume_up", '\u{E050}');
    pub const filter: Symbol = Symbol::new("filter", '\u{E3D3}');
    pub const palette: Symbol = Symbol::new("palette", '\u{E40A}');
    pub const flash_on: Symbol = Symbol::new("flash_on", '\u{E3E7}');
    pub const straighten: Symbol = Symbol::new("straighten", '\u{E41C}');
    pub const music_video: Symbol = Symbol::new("music_video", '\u{E063}');
    pub const search: Symbol = Symbol::new("search", '\u{E8B6}');
    pub const check: Symbol = Symbol::new("check", '\u{E5CA}');
    pub const close: Symbol = Symbol::new("close", '\u{E5CD}');
    pub const volume_off: Symbol = Symbol::new("volume_off", '\u{E04F}');
    pub const lock: Symbol = Symbol::new("lock", '\u{E897}');
    pub const lock_open: Symbol = Symbol::new("lock_open", '\u{E898}');
    pub const visibility: Symbol = Symbol::new("visibility", '\u{E8F4}');
    pub const visibility_off: Symbol = Symbol::new("visibility_off", '\u{E8F5}');
    pub const flag: Symbol = Symbol::new("flag", '\u{E153}');
    pub const transition: Symbol = Symbol::new("transition", '\u{EE09}');
    pub const speed: Symbol = Symbol::new("speed", '\u{E9E4}');
    pub const arrow_upward: Symbol = Symbol::new("arrow_upward", '\u{E5D8}');
    pub const arrow_downward: Symbol = Symbol::new("arrow_downward", '\u{E5DB}');
    pub const diamond: Symbol = Symbol::new("diamond", '\u{E19B}');
    pub const subtitle: Symbol = Symbol::new("subtitle", '\u{E061}');
    pub const text_fields: Symbol = Symbol::new("text_fields", '\u{E262}');
    pub const mask: Symbol = Symbol::new("mask", '\u{F330}');
    pub const font_download: Symbol = Symbol::new("font_download", '\u{E467}');
    pub const folder_open: Symbol = Symbol::new("folder_open", '\u{E2C8}');
    pub const save: Symbol = Symbol::new("save", '\u{E161}');
    pub const content_copy: Symbol = Symbol::new("content_copy", '\u{E14D}');
    pub const content_paste: Symbol = Symbol::new("content_paste", '\u{E14F}');
    pub const bookmark_add: Symbol = Symbol::new("bookmark_add", '\u{E598}');
    pub const bookmark_remove: Symbol = Symbol::new("bookmark_remove", '\u{E599}');
    pub const view_quilt: Symbol = Symbol::new("view_quilt", '\u{E8F1}');
}

/// Render an audio waveform visualization
pub fn audio_waveform(
    width: f32,
    height: f32,
    waveform_data: Option<&[f32]>,
    color: Color,
) -> View {
    let sample_count = ((width / 3.5).ceil() as usize).max(4).min(256);

    let samples: Vec<f32> = if let Some(data) = waveform_data {
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
        (0..sample_count)
            .map(|i| {
                let t = i as f32 / sample_count as f32;
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