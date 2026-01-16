pub mod assets;
pub mod editor;
pub mod timeline;

use crate::state::Store;
use repose_core::view::View;
use repose_core::Modifier;
use repose_ui::Surface;
use snapshort_ui_core::colors;
use std::rc::Rc;

pub fn root_view(store: Rc<Store>) -> View {
    Surface(
        Modifier::new().fill_max_size().background(colors::BG_DARK),
        editor::editor_screen(store),
    )
}
