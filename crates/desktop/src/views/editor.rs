use super::panels::{create_default_layout, create_panels};
use crate::state::Store;
use miniter_domain::ClipId;
use miniter_usecases::EditCommand;
use repose_core::prelude::theme;
use repose_core::{Color, Modifier, View};
use repose_docking::{DockArea, DockCallbacks};
use repose_material::material3;
use repose_material::Icon;
use repose_ui::{Box, Column, Row, Text, TextStyle, ViewExt};
use snapshort_ui_core::colors;
use snapshort_ui_core::Icons;
use snapshort_usecases::ProjectCommand;
use std::rc::Rc;

fn h_spacer(w: f32) -> View {
    Box(Modifier::new().width(w))
}
fn v_spacer(h: f32) -> View {
    Box(Modifier::new().height(h))
}

fn confirm_discard(store: &Store) -> bool {
    if !store.state.project_dirty.get() {
        return true;
    }
    let result = rfd::MessageDialog::new()
        .set_title("Unsaved Changes")
        .set_description("You have unsaved changes. Discard them?")
        .set_buttons(rfd::MessageButtons::YesNo)
        .show();
    matches!(result, rfd::MessageDialogResult::Yes)
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

    let main = Column(Modifier::new().fill_max_size()).child((
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
        .align_items(repose_core::AlignItems::CENTER))
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
            Modifier::new(),
            move || {
                if confirm_discard(&store_for_new) {
                    store_for_new.dispatch_project(ProjectCommand::Create {
                        name: "Untitled".to_string(),
                    });
                }
            },
            Default::default(),
            || Text("New"),
        ),
        h_spacer(8.0),
        material3::TextButton(
            Modifier::new(),
            move || {
                if confirm_discard(&store_for_open) {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        store_for_open.dispatch_project(ProjectCommand::Open { path });
                    }
                }
            },
            Default::default(),
            || Text("Open"),
        ),
        h_spacer(8.0),
        material3::TextButton(
            Modifier::new(),
            move || {
                let needs_save_as = store_for_save
                    .state
                    .project_path
                    .get()
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
                        let markers: Vec<_> = store_for_save
                            .state
                            .timeline_markers
                            .get()
                            .into_iter()
                            .map(|m| snapshort_usecases::TimelineMarkerData {
                                timestamp_us: m.timestamp_us,
                                label: m.label,
                            })
                            .collect();
                        store_for_save.dispatch_project(ProjectCommand::SaveAs { path, markers });
                    }
                } else {
                    let markers: Vec<_> = store_for_save
                        .state
                        .timeline_markers
                        .get()
                        .into_iter()
                        .map(|m| snapshort_usecases::TimelineMarkerData {
                            timestamp_us: m.timestamp_us,
                            label: m.label,
                        })
                        .collect();
                    store_for_save.dispatch_project(ProjectCommand::Save { markers });
                }
            },
            Default::default(),
            || Text("Save"),
        ),
        h_spacer(8.0),
        material3::TextButton(
            Modifier::new(),
            move || {
                *store_for_reset.dock_state.borrow_mut() = create_default_layout();
            },
            Default::default(),
            || Text("Reset Layout"),
        ),
        h_spacer(12.0),
        Text("Project Settings")
            .size(11.0)
            .color(th.on_surface_variant)
            .single_line(),
        h_spacer(12.0),
        Box(Modifier::new()
            .width(1.0)
            .height(14.0)
            .background(th.outline.with_alpha(128))),
        h_spacer(10.0),
        timeline_tools(store.clone()),
    ])
}

/// Timeline controls hosted in the editor chrome: split, marker, snap, zoom,
/// and current/total timecode.
fn timeline_tools(store: Rc<Store>) -> View {
    let th = theme();
    let playhead_tc = timecode_from_us(store.state.playhead.get().0);
    let total_tc = store
        .state
        .timeline
        .get()
        .map(|t| timecode_from_us(t.duration_end().as_micros()))
        .unwrap_or_else(|| "00:00".to_string());
    let snap = store.state.timeline_snap.get();
    let zoom = store.state.timeline_zoom.get();

    Row(Modifier::new().align_items(repose_core::AlignItems::CENTER)).child(vec![
        tool_icon_button(Icons::content_cut, {
            let store = store.clone();
            move || {
                if let (Some(clip_id), Some(_tl)) = (
                    store.state.selected_clip_id.get(),
                    store.state.timeline.get(),
                ) {
                    let at = store.state.playhead.get();
                    store.dispatch_edit(EditCommand::SplitClip {
                        clip_id,
                        at,
                        new_clip_id: ClipId::new(),
                    });
                }
            }
        }),
        tool_icon_button(Icons::flag, {
            let store = store.clone();
            move || {
                let at = store.state.playhead.get().0;
                let mut markers = store.state.timeline_markers.get();
                if !markers.iter().any(|m| m.timestamp_us == at) {
                    let label = format!("Mk{}", markers.len() + 1);
                    markers.push(crate::state::TimelineMarker {
                        timestamp_us: at,
                        label,
                    });
                    store.state.timeline_markers.set(markers);
                }
            }
        }),
        Box(Modifier::new()
            .height(28.0)
            .min_width(48.0)
            .background(if snap { th.primary_container } else { th.surface_variant })
            .clip_rounded(14.0)
            .padding_values(repose_core::PaddingValues {
                left: 12.0,
                right: 12.0,
                top: 0.0,
                bottom: 0.0,
            })
            .align_items(repose_core::AlignItems::CENTER)
            .justify_content(repose_core::AlignContent::CENTER)
            .clickable()
            .on_pointer_down({
                let store = store.clone();
                move |_| {
                    let current = store.state.timeline_snap.get();
                    store.state.timeline_snap.set(!current);
                }
            }))
        .child(
            Text("Snap")
                .size(12.0)
                .color(if snap { th.on_primary_container } else { th.on_surface_variant })
                .single_line(),
        ),
        h_spacer(8.0),
        Text("Zoom").size(10.0).color(th.on_surface_variant),
        h_spacer(4.0),
        material3::Slider(zoom, (0.5, 12.0), None, {
            let store = store.clone();
            move |value| store.state.timeline_zoom.set(value)
        }, Default::default())
        .modifier(Modifier::new().width(90.0).height(18.0)),
        h_spacer(10.0),
        Text(playhead_tc).size(11.0).color(colors::TEXT_ACCENT).single_line(),
        h_spacer(4.0),
        Text("/").size(11.0).color(th.on_surface_variant),
        h_spacer(4.0),
        Text(total_tc).size(11.0).color(th.on_surface).single_line(),
    ])
}

fn timecode_from_us(us: i64) -> String {
    let total_secs = us.max(0) / 1_000_000;
    let secs = total_secs % 60;
    let mins = (total_secs / 60) % 60;
    let hours = total_secs / 3600;
    if hours > 0 {
        format!("{hours:02}:{mins:02}:{secs:02}")
    } else {
        format!("{mins:02}:{secs:02}")
    }
}

fn tool_icon_button(icon: repose_material::Symbol, on_click: impl Fn() + 'static) -> View {
    let th = theme();
    Box(Modifier::new()
        .height(28.0)
        .min_width(32.0)
        .clip_rounded(14.0)
        .padding_values(repose_core::PaddingValues {
            left: 8.0,
            right: 8.0,
            top: 0.0,
            bottom: 0.0,
        })
        .align_items(repose_core::AlignItems::CENTER)
        .justify_content(repose_core::AlignContent::CENTER)
        .clickable()
        .on_pointer_down(move |_| on_click()))
    .child(Icon(icon).color(th.primary).size(16.0))
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
                .align_items(repose_core::AlignItems::CENTER)
                .justify_content(repose_core::AlignContent::CENTER),
        )
        .child((
            Box(Modifier::new().size(32.0, 32.0)).child(Icon(Icons::info).size(32.0)),
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
                .align_items(repose_core::AlignItems::CENTER)
                .justify_content(repose_core::AlignContent::CENTER),
        )
        .child((Box(Modifier::new()
            .fill_max_width()
            .max_width(360.0)
            .background(th.surface)
            .border(1.0, th.outline, 6.0)
            .padding(12.0))
        .child(Column(Modifier::new().fill_max_width()).child((
            Text("Something went wrong").size(12.0).color(th.on_surface),
            v_spacer(8.0),
            Text(message).size(11.0).color(th.on_surface_variant),
            v_spacer(12.0),
            material3::FilledTonalButton(
                Modifier::new(),
                move || store_for_close.state.last_error.set(None),
                Default::default(),
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
        .map(|p| p.meta.name.clone())
        .unwrap_or("No Project".to_string());
    let msg = store.state.status_msg.get();

    let timeline_info = store
        .state
        .timeline
        .get()
        .map(|tl| {
            let track_count = tl.tracks.len();
            let clip_count: usize = tl.tracks.iter().map(|t| t.clips.len()).sum();
            format!("Timeline: {track_count} tracks, {clip_count} clips")
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
        .align_items(repose_core::AlignItems::CENTER))
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
