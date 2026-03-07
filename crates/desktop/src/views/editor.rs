use super::panels::{create_default_layout, create_panels};
use crate::state::Store;
use repose_core::{Color, Modifier, View};
use repose_docking::{DockArea, DockCallbacks};
use repose_material::material3::FilledTonalButton;
use repose_ui::{Box, Column, Row, Stack, Text, TextStyle, ViewExt};
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
        menu_button("New", move || {
            store_for_new.dispatch_project(ProjectCommand::Create {
                name: "Untitled".to_string(),
            });
        }),
        h_spacer(8.0),
        menu_button("Open", move || {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Snapshort Project", &["snap"])
                .pick_file()
            {
                store_for_open.dispatch_project(ProjectCommand::Open { path });
            }
        }),
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
                    .map(|p| format!("{}.snap", p.name))
                    .unwrap_or_else(|| "project.snap".to_string());
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Snapshort Project", &["snap"])
                    .set_file_name(&default_name)
                    .save_file()
                {
                    store_for_save.dispatch_project(ProjectCommand::SaveAs { path });
                }
            } else {
                store_for_save.dispatch_project(ProjectCommand::Save);
            }
        }),
        Box(Modifier::new().flex_grow(1.0)),
        menu_button("Reset Layout", move || {
            *store_for_reset.dock_state.borrow_mut() = create_default_layout();
        }),
    ])
}

fn menu_button(label: &str, on_click: impl Fn() + 'static) -> View {
    let th = repose_core::theme();
    Box(Modifier::new()
        .height(24.0)
        .min_width(40.0)
        .clip_rounded(12.0)
        .padding_values(repose_core::PaddingValues {
            left: 12.0,
            right: 12.0,
            top: 0.0,
            bottom: 0.0,
        })
        .align_items(repose_core::AlignItems::Center)
        .justify_content(repose_core::JustifyContent::Center)
        .clickable()
        .on_pointer_down(move |_| on_click()))
    .child(Text(label).color(th.primary).size(12.0).single_line())
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
                FilledTonalButton("Dismiss".to_string(), move || {
                    store_for_close.state.last_error.set(None)
                }),
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
                "{}x{} | {}",
                tl.settings.resolution.width, tl.settings.resolution.height, fps_label
            )
        })
        .unwrap_or_else(|| "No Timeline".to_string());

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
        Text(project_name)
            .size(11.0)
            .color(colors::TEXT_MUTED)
            .single_line(),
        h_spacer(8.0),
        Box(Modifier::new()
            .width(1.0)
            .height(12.0)
            .background(colors::BORDER)),
        h_spacer(8.0),
        Text(timeline_info)
            .size(11.0)
            .color(colors::TEXT_MUTED)
            .single_line(),
        Box(Modifier::new().flex_grow(1.0)),
        Text(if msg.is_empty() { "Ready" } else { &msg })
            .size(11.0)
            .color(if msg.is_empty() {
                colors::TEXT_MUTED
            } else {
                colors::TEXT_ACCENT
            }),
    ])
}
