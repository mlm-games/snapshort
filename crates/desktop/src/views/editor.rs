use super::panels::{create_default_layout, create_panels};
use crate::state::Store;
use repose_core::prelude::theme;
use repose_core::{Color, Modifier, View};
use repose_docking::{DockArea, DockCallbacks};
use repose_material::material3;
use repose_ui::{Box, Column, Row, Stack, Text, TextStyle, ViewExt};
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

    let dock = DockArea(
        "main-dock",
        Modifier::new().fill_max_size(),
        dock_state,
        panels,
        DockCallbacks::default(),
    );

    let main = Stack(Modifier::new().fill_max_size()).child((
        dock,
        loading_overlay(store.clone()),
        error_overlay(store.clone()),
    ));

    Column(Modifier::new().fill_max_size()).child((
        menu_bar(store.clone()),
        main,
        status_bar(store),
    ))
}

fn menu_bar(store: Rc<Store>) -> View {
    let th = theme();

    let store_for_new = store.clone();
    let store_for_open = store.clone();
    let store_for_save = store.clone();
    let store_for_reset = store.clone();

    Row(Modifier::new()
        .fill_max_width()
        .height(36.0)
        .background(th.surface)
        .border(1.0, th.outline, 0.0)
        .padding_values(repose_core::PaddingValues {
            left: 12.0,
            right: 12.0,
            top: 4.0,
            bottom: 4.0,
        })
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        menu_item("File", th),
        menu_item("Edit", th),
        menu_item("Clip", th),
        menu_item("Sequence", th),
        menu_item("Marker", th),
        menu_item("Window", th),
        menu_item("Help", th),
        Box(Modifier::new().flex_grow(1.0)),
        material3::TextButton(
            move || {
                store_for_new.dispatch_project(ProjectCommand::Create {
                    name: "Untitled".to_string(),
                });
            },
            || Text("New"),
        ),
        h_spacer(8.0),
        material3::TextButton(
            move || {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    store_for_open.dispatch_project(ProjectCommand::Open { path });
                }
            },
            || Text("Open"),
        ),
        h_spacer(8.0),
        material3::TextButton(
            move || {
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
            },
            || Text("Save"),
        ),
        h_spacer(8.0),
        material3::TextButton(
            move || {
                *store_for_reset.dock_state.borrow_mut() = create_default_layout();
            },
            || Text("Reset Layout"),
        ),
        h_spacer(12.0),
        Text("Project Settings")
            .size(11.0)
            .color(th.on_surface_variant)
            .single_line(),
    ])
}

fn menu_item(label: &str, th: repose_core::prelude::Theme) -> View {
    Text(label)
        .size(12.0)
        .color(th.on_surface)
        .modifier(Modifier::new().padding_values(repose_core::PaddingValues {
            left: 8.0,
            right: 8.0,
            top: 4.0,
            bottom: 4.0,
        }))
}

fn empty_overlay() -> View {
    Box(Modifier::new().width(1.0).height(1.0))
}

fn loading_overlay(store: Rc<Store>) -> View {
    if !store.state.is_loading.get() {
        return empty_overlay();
    }

    let th = theme();

    Box(Modifier::new()
        .fill_max_size()
        .background(Color(0, 0, 0, 140))
        .z_index(200.0))
    .child(
        Column(
            Modifier::new()
                .fill_max_size()
                .align_items(repose_core::AlignItems::Center)
                .justify_content(repose_core::JustifyContent::Center),
        )
        .child((
            Box(Modifier::new().size(32.0, 32.0)).child(Text("⏳").size(32.0)),
            v_spacer(12.0),
            Text("Loading…").size(14.0).color(th.on_surface),
            v_spacer(6.0),
            Text("Working on background tasks")
                .size(11.0)
                .color(th.on_surface_variant),
        )),
    )
}

fn error_overlay(store: Rc<Store>) -> View {
    let Some(message) = store.state.last_error.get() else {
        return empty_overlay();
    };

    let th = theme();
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
            .background(th.surface)
            .border(1.0, th.outline, 6.0)
            .padding(12.0))
        .child(Column(Modifier::new().fill_max_width()).child((
            Text("Something went wrong").size(12.0).color(th.on_surface),
            v_spacer(8.0),
            Text(message).size(11.0).color(th.on_surface_variant),
            v_spacer(12.0),
            material3::FilledTonalButton(
                move || store_for_close.state.last_error.set(None),
                move || Text("Dismiss"),
            ),
        ))),)),
    )
}

fn status_bar(store: Rc<Store>) -> View {
    let th = theme();

    let project_name = store
        .state
        .project
        .get()
        .map(|p| p.name.clone())
        .unwrap_or("No Project".to_string());
    let msg = store.state.status_msg.get();

    let timeline_info = store
        .state
        .timeline
        .get()
        .map(|tl| {
            let fps = tl.settings.fps.as_f64();
            let fps_label = if (fps - fps.round()).abs() < 0.01 {
                format!("{:.0}fps", fps)
            } else {
                format!("{:.2}fps", fps)
            };
            format!(
                "Sequence: {} | {}x{} | {}",
                "Sequence 01",
                tl.settings.resolution.width,
                tl.settings.resolution.height,
                fps_label
            )
        })
        .unwrap_or_else(|| "No Timeline".to_string());

    Row(Modifier::new()
        .fill_max_width()
        .height(28.0)
        .background(th.surface)
        .border(1.0, th.outline, 0.0)
        .padding_values(repose_core::PaddingValues {
            left: 12.0,
            right: 12.0,
            top: 4.0,
            bottom: 4.0,
        })
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        Text(project_name)
            .size(11.0)
            .color(th.on_surface_variant)
            .single_line(),
        h_spacer(10.0),
        Box(Modifier::new()
            .width(1.0)
            .height(12.0)
            .background(th.outline.with_alpha(128))),
        h_spacer(10.0),
        Text(timeline_info)
            .size(11.0)
            .color(th.on_surface_variant)
            .single_line(),
        Box(Modifier::new().flex_grow(1.0)),
        Text(msg).size(11.0).color(th.primary),
        h_spacer(10.0),
        Box(Modifier::new()
            .width(1.0)
            .height(12.0)
            .background(th.outline.with_alpha(128))),
        h_spacer(10.0),
        Text("Ready").size(11.0).color(th.on_surface_variant),
    ])
}
