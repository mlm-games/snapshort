use super::panels::{create_default_layout, create_panels};
use crate::state::Store;
use repose_core::{Color, Modifier, View};
use repose_docking::{DockArea, DockCallbacks};
use repose_ui::{Box, Button, Column, Row, Stack, Text, TextStyle, ViewExt};
use snapshort_ui_core::colors;
use snapshort_usecases::ProjectCommand;
use std::rc::Rc;

fn h_spacer(w: f32) -> View {
    Box(Modifier::new().width(w))
}
fn v_spacer(h: f32) -> View {
    Box(Modifier::new().height(h))
}

pub fn editor_screen(store: Rc<Store>) -> View {
    let panels = create_panels(store.clone());
    let dock_state = store.dock_state.clone();

    // Main docking area
    let dock = DockArea(
        "main-dock",
        Modifier::new().fill_max_size(),
        dock_state,
        panels,
        DockCallbacks::default(),
    );

    // Wrap dock with loading/error overlays
    let main = Stack(Modifier::new().fill_max_size()).child((
        dock,
        loading_overlay(store.clone()),
        error_dialog(store.clone()),
    ));

    Column(Modifier::new().fill_max_size()).child((
        menu_bar(store.clone()),
        main,
        status_bar(store),
    ))
}

fn menu_bar(store: Rc<Store>) -> View {
    let store_for_new = store.clone();
    let store_for_open = store.clone();
    let store_for_save = store.clone();
    let store_for_reset = store.clone();

    Row(Modifier::new()
        .fill_max_width()
        .height(30.0)
        .background(colors::BG_PANEL)
        .padding_values(repose_core::PaddingValues {
            left: 10.0,
            right: 10.0,
            top: 2.0,
            bottom: 2.0,
        })
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        menu_item("File"),
        menu_item("Edit"),
        menu_item("Clip"),
        menu_item("Sequence"),
        menu_item("Marker"),
        menu_item("Graphics"),
        menu_item("Window"),
        menu_item("Help"),
        Box(Modifier::new().flex_grow(1.0)),
        menu_button("New", move || {
            store_for_new.dispatch_project(ProjectCommand::Create {
                name: "Untitled".to_string(),
            });
        })
        .modifier(Modifier::new().padding(2.0)),
        h_spacer(8.0),
        menu_button("Open", move || {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                store_for_open.dispatch_project(ProjectCommand::Open { path });
            }
        })
        .modifier(Modifier::new().padding(2.0)),
        h_spacer(8.0),
        menu_button("Save", move || {
            let needs_save_as = store_for_save
                .state
                .project
                .get()
                .and_then(|p| p.path)
                .is_none();
            if needs_save_as {
                let default_name = store_for_save
                    .state
                    .project
                    .get()
                    .map(|p| format!("{}.snap", p.id.0))
                    .unwrap_or_else(|| "project.snap".to_string());
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name(&default_name)
                    .save_file()
                {
                    store_for_save.dispatch_project(ProjectCommand::SaveAs { path });
                }
            } else {
                store_for_save.dispatch_project(ProjectCommand::Save);
            }
        })
        .modifier(Modifier::new().padding(2.0)),
        h_spacer(8.0),
        menu_button("Reset Layout", move || {
            // Reset dock layout to default
            *store_for_reset.dock_state.borrow_mut() = create_default_layout();
        })
        .modifier(Modifier::new().padding(2.0)),
        h_spacer(12.0),
        Text("Project Settings")
            .size(11.0)
            .color(colors::TEXT_MUTED),
    ])
}

fn menu_item(label: &str) -> View {
    Text(label).size(12.0).color(colors::TEXT_PRIMARY).modifier(
        Modifier::new()
            .padding_values(repose_core::PaddingValues {
                left: 6.0,
                right: 6.0,
                top: 2.0,
                bottom: 2.0,
            })
            .on_pointer_enter(|_| {}),
    )
}

fn menu_button(label: &str, on_click: impl Fn() + 'static) -> View {
    Button(Text(label).size(11.0).color(colors::TEXT_PRIMARY), on_click).modifier(
        Modifier::new()
            .padding_values(repose_core::PaddingValues {
                left: 8.0,
                right: 8.0,
                top: 4.0,
                bottom: 4.0,
            })
            .background(colors::BG_HEADER)
            .clip_rounded(4.0),
    )
}

fn empty_overlay() -> View {
    Box(Modifier::new().width(1.0).height(1.0))
}

fn loading_overlay(store: Rc<Store>) -> View {
    if !store.state.is_loading.get() {
        return empty_overlay();
    }

    Box(Modifier::new()
        .fill_max_size()
        .background(Color(0, 0, 0, 160))
        .z_index(200.0))
    .child(
        Column(
            Modifier::new()
                .fill_max_size()
                .align_items(repose_core::AlignItems::Center)
                .justify_content(repose_core::JustifyContent::Center),
        )
        .child((
            Text("Loading…").size(14.0).color(colors::TEXT_PRIMARY),
            v_spacer(6.0),
            Text("Working on background tasks")
                .size(11.0)
                .color(colors::TEXT_MUTED),
        )),
    )
}

fn error_dialog(store: Rc<Store>) -> View {
    let Some(message) = store.state.last_error.get() else {
        return empty_overlay();
    };

    let store_for_close = store.clone();

    Box(Modifier::new()
        .fill_max_size()
        .background(Color(0, 0, 0, 180))
        .z_index(300.0))
    .child(
        Column(
            Modifier::new()
                .fill_max_size()
                .align_items(repose_core::AlignItems::Center)
                .justify_content(repose_core::JustifyContent::Center),
        )
        .child((Box(Modifier::new()
            .width(360.0)
            .background(colors::BG_PANEL)
            .border(1.0, colors::BORDER, 6.0)
            .padding(12.0))
        .child(
            Column(Modifier::new().fill_max_width()).child((
                Text("Something went wrong")
                    .size(12.0)
                    .color(colors::TEXT_PRIMARY),
                v_spacer(8.0),
                Text(message).size(11.0).color(colors::TEXT_MUTED),
                v_spacer(12.0),
                Button(
                    Text("Dismiss").size(11.0).color(colors::TEXT_PRIMARY),
                    move || store_for_close.state.last_error.set(None),
                )
                .modifier(
                    Modifier::new()
                        .padding(6.0)
                        .background(colors::BG_HEADER)
                        .clip_rounded(4.0),
                ),
            )),
        ),)),
    )
}

fn status_bar(store: Rc<Store>) -> View {
    let project_name = store
        .state
        .project
        .get()
        .map(|p| p.name.clone())
        .unwrap_or("No Project".to_string());
    let msg = store.state.status_msg.get();

    Row(Modifier::new()
        .fill_max_width()
        .height(26.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding_values(repose_core::PaddingValues {
            left: 10.0,
            right: 10.0,
            top: 2.0,
            bottom: 2.0,
        })
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        Text(project_name).size(11.0).color(colors::TEXT_MUTED),
        h_spacer(8.0),
        Box(Modifier::new()
            .width(1.0)
            .height(12.0)
            .background(colors::BORDER)),
        h_spacer(8.0),
        Text("Sequence: Sequence 01 | 1920x1080 | 24fps")
            .size(11.0)
            .color(colors::TEXT_MUTED),
        Box(Modifier::new().flex_grow(1.0)),
        Text(msg).size(11.0).color(colors::TEXT_ACCENT),
        h_spacer(8.0),
        Box(Modifier::new()
            .width(1.0)
            .height(12.0)
            .background(colors::BORDER)),
        h_spacer(8.0),
        Text("Ready").size(11.0).color(colors::TEXT_MUTED),
    ])
}
