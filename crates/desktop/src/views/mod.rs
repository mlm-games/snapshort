pub mod assets;
pub mod dnd;
pub mod editor;
pub mod panels;
pub mod timeline;

use crate::state::Store;
use miniter_usecases::EditCommand;
use repose_core::input::Key;
use repose_core::{scoped_effect, shortcuts, Dispose, Modifier, View};
use repose_core::locals::set_theme_default;
use repose_core::prelude::Theme;
use repose_ui::Surface;
use snapshort_usecases::ProjectCommand;
use std::rc::Rc;

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
                        if store_for_shortcuts.state.timeline.get().is_some() {
                            let at = store_for_shortcuts.state.playhead.get();
                            store_for_shortcuts.dispatch_edit(EditCommand::SplitClip {
                                clip_id,
                                at,
                                new_clip_id: miniter_domain::ClipId::new(),
                            });
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

    set_theme_default(Theme::default());

    Surface(
        Modifier::new().fill_max_size(),
        editor::editor_screen(store),
    )
}
