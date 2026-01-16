use eframe::egui::{self, Color32, Rounding, Stroke};

pub struct EditorColors {
    pub bg_dark: Color32,
    pub bg_medium: Color32,
    pub bg_light: Color32,
    pub accent: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
}

impl Default for EditorColors {
    fn default() -> Self {
        Self {
            bg_dark: Color32::from_rgb(24, 24, 28),
            bg_medium: Color32::from_rgb(32, 32, 38),
            bg_light: Color32::from_rgb(44, 44, 52),
            accent: Color32::from_rgb(88, 166, 255),
            text_primary: Color32::from_rgb(240, 240, 245),
            text_secondary: Color32::from_rgb(180, 180, 190),
            text_muted: Color32::from_rgb(120, 120, 130),
        }
    }
}

pub static COLORS: std::sync::LazyLock<EditorColors> =
    std::sync::LazyLock::new(EditorColors::default);

pub fn setup_custom_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(12.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);

    style.visuals.window_rounding = Rounding::same(8.0);
    style.visuals.widgets.noninteractive.rounding = Rounding::same(4.0);
    style.visuals.widgets.inactive.rounding = Rounding::same(4.0);
    style.visuals.widgets.hovered.rounding = Rounding::same(4.0);
    style.visuals.widgets.active.rounding = Rounding::same(4.0);

    let colors = &*COLORS;

    style.visuals.dark_mode = true;
    style.visuals.panel_fill = colors.bg_medium;
    style.visuals.window_fill = colors.bg_medium;
    style.visuals.extreme_bg_color = colors.bg_dark;

    style.visuals.widgets.inactive.bg_fill = colors.bg_light;
    style.visuals.widgets.hovered.bg_fill = colors.accent;

    ctx.set_style(style);
}
