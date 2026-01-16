pub mod assets;
pub mod editor;
pub mod timeline;
pub mod welcome;

use crate::state::{Store, ViewMode};
use repose_core::view::View;
use repose_core::Modifier;
use repose_ui::Surface;
use snapshort_ui_core::colors;
use std::rc::Rc;

pub fn root_view(store: Rc<Store>) -> View {
    let mode = store.state.current_view.get();

    Surface(
        Modifier::new().fill_max_size().background(colors::BG_DARK),
        match mode {
            ViewMode::Welcome => welcome::welcome_screen(store),
            ViewMode::Editor => editor::editor_screen(store),
        },
    )
}
