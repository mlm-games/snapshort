pub mod assets;
pub mod dnd;
pub mod editor;
pub mod inspector;
pub mod panels;
pub mod timeline;

use crate::state::Store;
use miniter_usecases::EditCommand;
use repose_core::input::Key;
use repose_core::{scoped_effect, shortcuts, Dispose, Modifier, View};
use repose_core::locals::set_theme_default;
use repose_core::prelude::{theme, Theme};
use repose_core::{Color, ColorScheme};
use repose_ui::Box;
use repose_ui::ViewExt;
use snapshort_usecases::ProjectCommand;
use std::rc::Rc;

fn snapshort_theme() -> Theme {
    let colors = ColorScheme {
        primary: Color::from_hex("#4C9AFF"),
        on_primary: Color::from_hex("#001D36"),
        primary_container: Color::from_hex("#0A3D6B"),
        on_primary_container: Color::from_hex("#D6E9FF"),

        secondary: Color::from_hex("#9BB0C9"),
        on_secondary: Color::from_hex("#1A2A3A"),
        secondary_container: Color::from_hex("#2A3A4A"),
        on_secondary_container: Color::from_hex("#D0DEEE"),

        tertiary: Color::from_hex("#7DDAA0"),
        on_tertiary: Color::from_hex("#00391F"),
        tertiary_container: Color::from_hex("#0A4A2E"),
        on_tertiary_container: Color::from_hex("#B8F5D0"),

        error: Color::from_hex("#FF6B7A"),
        on_error: Color::from_hex("#3B0008"),
        error_container: Color::from_hex("#8C1D18"),
        on_error_container: Color::from_hex("#FFDAD6"),

        background: Color::from_hex("#12141A"),
        on_background: Color::from_hex("#E8EAED"),
        surface: Color::from_hex("#1A1D24"),
        on_surface: Color::from_hex("#E8EAED"),
        surface_variant: Color::from_hex("#2A2F3A"),
        on_surface_variant: Color::from_hex("#A8B0BD"),
        surface_container_lowest: Color::from_hex("#0C0E12"),
        surface_container_low: Color::from_hex("#161920"),
        surface_container: Color::from_hex("#1C2028"),
        surface_container_high: Color::from_hex("#242933"),
        surface_container_highest: Color::from_hex("#2E3440"),
        surface_bright: Color::from_hex("#2A303C"),
        surface_dim: Color::from_hex("#12141A"),
        surface_tint: Color::from_hex("#4C9AFF"),

        inverse_surface: Color::from_hex("#E8EAED"),
        inverse_on_surface: Color::from_hex("#1A1D24"),
        inverse_primary: Color::from_hex("#0A5CA8"),

        outline: Color::from_hex("#3D4450"),
        outline_variant: Color::from_hex("#2A2F3A"),

        scrim: Color::from_hex("#000000"),
        shadow: Color::from_hex("#000000"),
        focus: Color::from_hex("#4C9AFF"),
    };

    Theme::default().with_colors(colors)
}

pub fn root_view(store: Rc<Store>) -> View {
    let store_for_shortcuts = store.clone();
    scoped_effect(move || {
        let handler_scope =
            shortcuts::InstallShortcutHandler(Rc::new(move |action| match action {
                shortcuts::Action::Save => {
                    let markers: Vec<_> = store_for_shortcuts
                        .state
                        .timeline_markers
                        .get()
                        .into_iter()
                        .map(|m| snapshort_usecases::TimelineMarkerData {
                            timestamp_us: m.timestamp_us,
                            label: m.label,
                        })
                        .collect();
                    store_for_shortcuts
                        .dispatch_project(ProjectCommand::Save { markers });
                    true
                }
                shortcuts::Action::Undo => {
                    store_for_shortcuts.dispatch_undo();
                    true
                }
                shortcuts::Action::Redo => {
                    store_for_shortcuts.dispatch_redo();
                    true
                }
                shortcuts::Action::Copy => {
                    store_for_shortcuts.copy_selected_clip();
                    true
                }
                shortcuts::Action::Cut => {
                    store_for_shortcuts.cut_selected_clip();
                    true
                }
                shortcuts::Action::Paste => {
                    store_for_shortcuts.paste_clip();
                    true
                }
                shortcuts::Action::Custom(name) if name.as_ref() == "timeline:delete" => {
                    if let Some(clip_id) = store_for_shortcuts.state.selected_clip_id.get() {
                        store_for_shortcuts
                            .dispatch_edit(EditCommand::RemoveClip { clip_id });
                        store_for_shortcuts.state.selected_clip_id.set(None);
                    }
                    true
                }
                shortcuts::Action::Custom(name) if name.as_ref() == "timeline:split" => {
                    if let Some(clip_id) = store_for_shortcuts.state.selected_clip_id.get() {
                        if let Some(timeline) = store_for_shortcuts.state.timeline.get() {
                            let at = store_for_shortcuts.state.playhead.get();
                            if crate::views::timeline::can_split_clip(&timeline, clip_id, at) {
                                store_for_shortcuts.dispatch_edit(EditCommand::SplitClip {
                                    clip_id,
                                    at,
                                    new_clip_id: miniter_domain::ClipId::new(),
                                });
                            }
                        }
                    }
                    true
                }
                _ => false,
            }));

        let delete_map = shortcuts::ShortcutMap::new()
            .bind(
                repose_core::input::Key::Delete,
                repose_core::input::Modifiers::default(),
                shortcuts::Action::Custom("timeline:delete".into()),
            )
            .bind(
                repose_core::input::Key::Backspace,
                repose_core::input::Modifiers::default(),
                shortcuts::Action::Custom("timeline:delete".into()),
            )
            .bind(
                Key::Character('s'),
                repose_core::input::Modifiers::default(),
                shortcuts::Action::Custom("timeline:split".into()),
            )
            .bind(
                Key::Character('S'),
                repose_core::input::Modifiers::default(),
                shortcuts::Action::Custom("timeline:split".into()),
            );
        let map_scope = shortcuts::InstallShortcutMap(delete_map);

        Dispose::new(move || {
            map_scope.run();
            handler_scope.run();
        })
    });

    set_theme_default(snapshort_theme());

    let overlay = store.overlay.clone();

    let content = Box(
        Modifier::new()
            .fill_max_size()
            .background(theme().surface_container_lowest),
    )
    .child(editor::editor_screen(store));

    overlay.host(Modifier::new().fill_max_size(), content)
}
