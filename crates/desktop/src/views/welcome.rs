use crate::state::Store;
use repose_core::{Color, Modifier, View};
use repose_ui::{Column, Spacer, Text, TextStyle, ViewExt};
use snapshort_ui_core::{colors, primary_button};
use snapshort_usecases::ProjectCommand;
use std::rc::Rc;

pub fn welcome_screen(store: Rc<Store>) -> View {
    Column(
        Modifier::new()
            .fill_max_size()
            .justify_content(repose_core::JustifyContent::Center)
            .align_items(repose_core::AlignItems::Center),
    )
    .child(Text("Snapshort").size(48.0).color(colors::ACCENT))
    .child(
        Text("Professional Video Editor")
            .size(16.0)
            .color(colors::TEXT_MUTED),
    )
    .child(Spacer().modifier(Modifier::new().max_height(40.0)))
    .child(
        primary_button("New Project", {
            let s = store.clone();
            move || {
                s.dispatch_project(ProjectCommand::Create {
                    name: "Untitled".to_string(),
                })
            }
        })
        .modifier(Modifier::new().width(200.0)),
    )
}
