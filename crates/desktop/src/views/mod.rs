pub mod assets;
pub mod dnd;
pub mod editor;
pub mod panels;
pub mod timeline;

use crate::state::Store;
use repose_core::{shortcuts, Modifier, View};
use repose_ui::Surface;
use snapshort_ui_core::colors;
use snapshort_usecases::{ProjectCommand, TimelineCommand};
use std::rc::Rc;

pub fn root_view(store: Rc<Store>) -> View {
    let store_for_shortcuts = store.clone();
    let _dispose = shortcuts::InstallShortcutHandler(Rc::new(move |action| match action {
        shortcuts::Action::Save => {
            store_for_shortcuts.dispatch_project(ProjectCommand::Save);
            true
        }
        shortcuts::Action::Undo => {
            store_for_shortcuts.dispatch_timeline(TimelineCommand::Undo);
            true
        }
        shortcuts::Action::Redo => {
            store_for_shortcuts.dispatch_timeline(TimelineCommand::Redo);
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
                store_for_shortcuts.dispatch_timeline(TimelineCommand::RippleDelete { clip_id });
                store_for_shortcuts.state.selected_clip_id.set(None);
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
        );
    let _delete_scope = shortcuts::InstallShortcutMap(delete_map);

    Surface(
        Modifier::new().fill_max_size().background(colors::BG_DARK),
        editor::editor_screen(store),
    )
}
