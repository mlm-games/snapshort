use super::chrome::{HRule, HSpace, StatusChip, ToolIcon, VSpace};
use super::panels::{create_default_layout, create_panels};
use crate::state::{DiscardPending, Store};
use miniter_domain::{ClipId, Timestamp};
use miniter_usecases::EditCommand;
use repose_core::prelude::theme;
use repose_core::{
    AlignItems, Color, Dp, JustifyContent, Modifier, PaddingValues, Sp, View, remember_with_key,
};
use repose_docking::{DockArea, DockCallbacks};
use repose_material::Icon;
use repose_material::material3;
use repose_material::material3::{
    ButtonConfig, Card, CardConfig, CircularProgressIndicator, Dialog, DialogProperties,
    DialogState, FilledTonalButton, Scaffold, ScaffoldConfig, Snackbar, SnackbarConfig, TextButton,
};
use repose_ui::overlay::{SnackbarController, SnackbarRequest};
use repose_ui::{Box, Column, Row, Text, TextStyle, ViewExt, ZStack};
use snapshort_ui_core::Icons;
use snapshort_usecases::ProjectCommand;
use std::rc::Rc;

pub(crate) fn markers_for_save(store: &Store) -> Vec<snapshort_usecases::TimelineMarkerData> {
    store
        .state
        .timeline_markers
        .get()
        .into_iter()
        .map(|m| snapshort_usecases::TimelineMarkerData {
            timestamp_us: m.timestamp_us,
            label: m.label,
        })
        .collect()
}

pub(crate) fn project_command_create() -> ProjectCommand {
    ProjectCommand::Create {
        name: "Untitled".to_string(),
    }
}

pub(crate) fn project_command_open(path: std::path::PathBuf) -> ProjectCommand {
    ProjectCommand::Open { path }
}

pub(crate) fn project_command_save(store: &Store) -> ProjectCommand {
    ProjectCommand::Save {
        markers: markers_for_save(store),
    }
}

/// Start the Save As flow: the saver picker resolves to a SaveAs command
/// (desktop/Android paths) or a direct download (web) via the UI pump.
pub(crate) fn start_save_as_picker(store: &Store) {
    let default_name = store
        .state
        .project
        .get()
        .map(|p| format!("{}.snap", p.id.0))
        .unwrap_or_else(|| "project.snap".to_string());
    let markers = markers_for_save(store);
    crate::pickers::start_picker(
        store,
        crate::pickers::ActivePicker::SaveProject {
            picker: crate::pickers::pick_save_project(&default_name),
            markers,
        },
    );
}

pub(crate) fn asset_command_import(
    paths: Vec<std::path::PathBuf>,
) -> snapshort_usecases::AssetCommand {
    snapshort_usecases::AssetCommand::Import { paths }
}

/// Run a New/Open intent that was staged behind the discard dialog.
fn run_pending(store: &Store, pending: DiscardPending) {
    match pending {
        DiscardPending::New => store.dispatch_project(project_command_create()),
        DiscardPending::Open => {
            crate::pickers::start_picker(
                store,
                crate::pickers::ActivePicker::OpenProject(crate::pickers::pick_open_project()),
            );
        }
    }
}

pub fn editor_screen(store: Rc<Store>) -> View {
    // In-app error feedback (renamite shell.rs pattern): consume last_error
    // into a snackbar instead of a blocking overlay.
    show_error_snackbar(&store);

    let panels = create_panels(store.clone());
    let dock_state = store.dock_state.clone();

    let s_body = store.clone();
    let s_top = store.clone();
    let s_bottom = store.clone();

    Scaffold(
        move |_| {
            Column(Modifier::new().fill_max_size()).child((
                tools_row(s_body.clone()),
                ZStack(Modifier::new().fill_max_size().flex_grow(1.0)).child((
                    DockArea(
                        "main-dock",
                        Modifier::new().fill_max_size(),
                        dock_state.clone(),
                        panels.clone(),
                        DockCallbacks::default(),
                    ),
                    loading_overlay(s_body.clone()),
                    discard_dialog(s_body.clone()),
                )),
            ))
        },
        ScaffoldConfig {
            top_bar: Some(app_top_bar(s_top)),
            bottom_bar: Some(transport_and_status(s_bottom)),
            container_color: theme().surface_container_lowest,
            ..Default::default()
        },
    )
}

/// Project chrome: menus + dirty pill + save actions. Editing tools live in
/// the second row ([`tools_row`]), transport in the bottom bar.
fn app_top_bar(store: Rc<Store>) -> View {
    let th = theme();

    let store_for_save = store.clone();
    let store_for_save_as = store.clone();
    let store_for_reset = store.clone();

    let tm = store.state.top_menus.clone();
    let dirty = store.state.project_dirty.get();
    let name = store
        .state
        .project
        .get()
        .map(|p| p.meta.name.clone())
        .unwrap_or_else(|| "Untitled".to_string());

    Row(Modifier::new()
        .fill_max_width()
        .height(Dp(64.0))
        .background(th.surface_container)
        .padding_values(PaddingValues {
            left: Dp(8.0),
            right: Dp(12.0),
            top: Dp(0.0),
            bottom: Dp(0.0),
        })
        .align_items(AlignItems::CENTER)
        .gap(Dp(4.0)))
    .child(vec![
        super::timeline::menus::top_menu_dropdown(
            &store,
            "File",
            tm.file.clone(),
            super::timeline::menus::file_menu_items(&store),
        ),
        super::timeline::menus::top_menu_dropdown(
            &store,
            "Edit",
            tm.edit.clone(),
            super::timeline::menus::edit_menu_items(&store),
        ),
        super::timeline::menus::top_menu_dropdown(
            &store,
            "Clip",
            tm.clip.clone(),
            super::timeline::menus::clip_top_menu_items(&store),
        ),
        super::timeline::menus::top_menu_dropdown(
            &store,
            "Sequence",
            tm.sequence.clone(),
            super::timeline::menus::sequence_menu_items(&store),
        ),
        super::timeline::menus::top_menu_dropdown(
            &store,
            "Marker",
            tm.marker.clone(),
            super::timeline::menus::marker_menu_items(&store),
        ),
        super::timeline::menus::top_menu_dropdown(
            &store,
            "Window",
            tm.window.clone(),
            super::timeline::menus::window_menu_items(&store),
        ),
        super::timeline::menus::top_menu_dropdown(
            &store,
            "Help",
            tm.help.clone(),
            super::timeline::menus::help_menu_items(&store),
        ),
        Box(Modifier::new().flex_grow(1.0)),
        StatusChip(if dirty { format!("{name} •") } else { name }, dirty),
        HSpace(8.0),
        FilledTonalButton(
            Modifier::new().height(Dp(36.0)),
            move || {
                store_for_save.dispatch_project(project_command_save(&store_for_save));
            },
            ButtonConfig {
                enabled: true,
                height: Dp(36.0),
                content_padding: Some(PaddingValues {
                    left: Dp(14.0),
                    right: Dp(14.0),
                    top: Dp(0.0),
                    bottom: Dp(0.0),
                }),
                ..Default::default()
            },
            move || {
                Row(Modifier::new().align_items(AlignItems::CENTER).gap(Dp(6.0))).child((
                    Icon(Icons::save).size(Sp(18.0)),
                    Text(if dirty { "Save*" } else { "Save" }).size(theme().typography.label_large),
                ))
            },
        ),
        TextButton(
            Modifier::new().height(Dp(36.0)),
            move || {
                start_save_as_picker(&store_for_save_as);
            },
            ButtonConfig {
                height: Dp(36.0),
                ..Default::default()
            },
            || Text("Save as").size(theme().typography.label_large),
        ),
        ToolIcon(
            "reset_layout",
            Icons::view_quilt,
            "Reset layout",
            false,
            true,
            move || {
                *store_for_reset.dock_state.borrow_mut() = create_default_layout();
            },
        ),
    ])
}

/// Editing tools: split, marker, snap, zoom. Split off the menu bar into its
/// own chrome row so File/Edit never fight editing controls.
fn tools_row(store: Rc<Store>) -> View {
    let th = theme();
    let snap = store.state.timeline_snap.get();
    let zoom = store.state.timeline_zoom.get();

    let split_enabled = store
        .state
        .selected_clip_id
        .get()
        .zip(store.state.timeline.get())
        .map(|(id, tl)| super::timeline::can_split_clip(&tl, id, store.state.playhead.get()))
        .unwrap_or(false);

    Row(Modifier::new()
        .fill_max_width()
        .height(Dp(48.0))
        .background(th.surface_container_low)
        .padding_values(PaddingValues {
            left: Dp(12.0),
            right: Dp(12.0),
            top: Dp(4.0),
            bottom: Dp(4.0),
        })
        .align_items(AlignItems::CENTER)
        .gap(Dp(8.0)))
    .child(vec![
        ToolIcon(
            "split",
            Icons::content_cut,
            "Split at playhead (S)",
            false,
            split_enabled,
            {
                let store = store.clone();
                move || {
                    if let (Some(clip_id), Some(timeline)) = (
                        store.state.selected_clip_id.get(),
                        store.state.timeline.get(),
                    ) {
                        let at = store.state.playhead.get();
                        if super::timeline::can_split_clip(&timeline, clip_id, at) {
                            store.dispatch_edit(EditCommand::SplitClip {
                                clip_id,
                                at,
                                new_clip_id: ClipId::new(),
                            });
                        }
                    }
                }
            },
        ),
        ToolIcon(
            "marker",
            Icons::flag,
            "Add marker at playhead",
            false,
            true,
            {
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
            },
        ),
        HRule(),
        material3::FilterChip(
            snap,
            {
                let store = store.clone();
                move || {
                    let current = store.state.timeline_snap.get();
                    store.state.timeline_snap.set(!current);
                }
            },
            Text("Snap").size(theme().typography.label_large),
            Some(Icon(Icons::straighten).size(Sp(16.0))),
            None,
            Default::default(),
        ),
        HSpace(4.0),
        Text("Zoom")
            .size(theme().typography.label_large)
            .color(th.on_surface_variant),
        material3::Slider(
            zoom,
            (0.5, 12.0),
            None,
            {
                let store = store.clone();
                move |value| store.state.timeline_zoom.set(value)
            },
            Default::default(),
        )
        .modifier(Modifier::new().width(Dp(120.0))),
    ])
}

/// Bottom bar (80dp): transport cluster + timecode pill over a thin project
/// status strip.
fn transport_and_status(store: Rc<Store>) -> View {
    let th = theme();
    let can_undo = store.state.can_undo.get();
    let can_redo = store.state.can_redo.get();
    let is_playing = store.state.playback_state.get() == "Playing";
    let playhead_tc = timecode_from_us(store.state.playhead.get().0);
    let total_tc = store
        .state
        .timeline
        .get()
        .map(|t| timecode_from_us(t.duration_end().as_micros()))
        .unwrap_or_else(|| "00:00".to_string());

    let project_name = store
        .state
        .project
        .get()
        .map(|p| p.meta.name.clone())
        .unwrap_or_else(|| "No Project".to_string());
    let msg = store.state.status_msg.get();
    let bg_jobs = store.state.background_jobs.get();
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

    let s_undo = store.clone();
    let s_redo = store.clone();

    Column(
        Modifier::new()
            .fill_max_width()
            .height(Dp(80.0))
            .background(th.surface_container),
    )
    .child((
        Row(Modifier::new()
            .fill_max_width()
            .height(Dp(56.0))
            .padding_values(PaddingValues {
                left: Dp(16.0),
                right: Dp(16.0),
                top: Dp(0.0),
                bottom: Dp(0.0),
            })
            .align_items(AlignItems::CENTER)
            .gap(Dp(8.0)))
        .child(vec![
            ToolIcon("undo", Icons::undo, "Undo", false, can_undo, move || {
                s_undo.dispatch_undo()
            }),
            ToolIcon("redo", Icons::redo, "Redo", false, can_redo, move || {
                s_redo.dispatch_redo()
            }),
            Box(Modifier::new().flex_grow(1.0)),
            transport_button(
                store.clone(),
                Icons::skip_previous,
                "Go to start",
                snapshort_usecases::PlaybackCommand::Seek {
                    timestamp: Timestamp(0),
                },
            ),
            transport_button_rel(store.clone(), Icons::fast_rewind, "Back 1s", -1_000_000),
            play_button(store.clone(), is_playing),
            transport_button(
                store.clone(),
                Icons::stop,
                "Stop",
                snapshort_usecases::PlaybackCommand::Stop,
            ),
            transport_button_rel(store.clone(), Icons::fast_forward, "Forward 1s", 1_000_000),
            Box(Modifier::new().flex_grow(1.0)),
            Box(Modifier::new()
                .height(Dp(36.0))
                .padding_values(PaddingValues {
                    left: Dp(12.0),
                    right: Dp(12.0),
                    top: Dp(0.0),
                    bottom: Dp(0.0),
                })
                .background(th.surface_container_highest)
                .clip_rounded(Dp(10.0))
                .align_items(AlignItems::CENTER)
                .justify_content(repose_core::AlignContent::CENTER))
            .child(
                Row(Modifier::new().gap(Dp(6.0)).align_items(AlignItems::CENTER)).child((
                    Text(playhead_tc)
                        .size(theme().typography.title_small)
                        .color(th.primary)
                        .single_line(),
                    Text("/")
                        .size(theme().typography.body_medium)
                        .color(th.on_surface_variant),
                    Text(total_tc)
                        .size(theme().typography.title_small)
                        .color(th.on_surface)
                        .single_line(),
                )),
            ),
        ]),
        Row(Modifier::new()
            .fill_max_width()
            .height(Dp(24.0))
            .padding_values(PaddingValues {
                left: Dp(12.0),
                right: Dp(12.0),
                top: Dp(0.0),
                bottom: Dp(0.0),
            })
            .align_items(AlignItems::CENTER)
            .gap(Dp(10.0)))
        .child(vec![
            Text(format!(
                "{}{}",
                project_name,
                if store.state.project_dirty.get() {
                    " •"
                } else {
                    ""
                }
            ))
            .size(theme().typography.label_small)
            .color(th.on_surface_variant)
            .single_line(),
            Text(timeline_info)
                .size(theme().typography.label_small)
                .color(th.on_surface_variant)
                .single_line(),
            Box(Modifier::new().flex_grow(1.0)),
            Text(msg)
                .size(theme().typography.label_small)
                .color(th.primary)
                .single_line(),
            if bg_jobs > 0 {
                Text(format!("{} background job(s)", bg_jobs))
                    .size(theme().typography.label_small)
                    .color(th.primary)
                    .single_line()
            } else {
                empty_overlay()
            },
        ]),
    ))
}

fn transport_button(
    store: Rc<Store>,
    icon: repose_material::Symbol,
    tooltip: &'static str,
    cmd: snapshort_usecases::PlaybackCommand,
) -> View {
    ToolIcon(
        format!("transport_{tooltip}"),
        icon,
        tooltip,
        false,
        true,
        move || store.dispatch_playback(cmd.clone()),
    )
}

fn transport_button_rel(
    store: Rc<Store>,
    icon: repose_material::Symbol,
    tooltip: &'static str,
    delta_us: i64,
) -> View {
    ToolIcon(
        format!("transport_{tooltip}"),
        icon,
        tooltip,
        false,
        true,
        move || {
            let cur = store.state.playhead.get().0;
            store.dispatch_playback(snapshort_usecases::PlaybackCommand::Seek {
                timestamp: Timestamp((cur + delta_us).max(0)),
            });
        },
    )
}

/// Primary transport: 48dp filled play/pause.
fn play_button(store: Rc<Store>, is_playing: bool) -> View {
    let th = theme();
    let icon = if is_playing {
        Icon(Icons::pause).size(Sp(22.0))
    } else {
        Icon(Icons::play_arrow).size(Sp(22.0))
    };
    let cmd = if is_playing {
        snapshort_usecases::PlaybackCommand::Pause
    } else {
        snapshort_usecases::PlaybackCommand::Play
    };
    material3::FilledIconButton(
        icon,
        move || store.dispatch_playback(cmd.clone()),
        material3::IconButtonConfig {
            container_size: Some(Dp(48.0)),
            colors: material3::IconButtonColors {
                container_color: th.primary,
                content_color: th.on_primary,
                disabled_container_color: th.on_surface.with_alpha(30),
                disabled_content_color: th.on_surface.with_alpha(90),
            },
            ..Default::default()
        },
    )
}

/// In-app unsaved-changes dialog (M3), driven by staged [`DiscardPending`].
fn discard_dialog(store: Rc<Store>) -> View {
    let state = remember_with_key("discard_dialog", DialogState::new);
    if store.state.confirm_discard.get().is_some() {
        state.show();
    } else {
        state.dismiss();
    }
    let Some(pending) = store.state.confirm_discard.get() else {
        // Keep the Dialog mounted so its exit animation can finish; it
        // renders nothing while dismissed.
        return Dialog(
            state.clone(),
            store.overlay.clone(),
            Modifier::new(),
            DialogProperties::default(),
            Box(Modifier::new()),
        );
    };

    let th = theme();
    let label = th.typography.label_large;

    let s_cancel = store.clone();
    let s_drop = store.clone();
    let s_save = store.clone();
    let pending_drop = pending.clone();
    let pending_save = pending.clone();

    let content = Column(
        Modifier::new()
            .padding(Dp(24.0))
            .gap(Dp(12.0))
            .fill_max_width(),
    )
    .child((
        Text("Unsaved changes").size(th.typography.headline_small),
        Text("Save changes to this project before continuing?")
            .size(th.typography.body_medium)
            .color(th.on_surface_variant),
        VSpace(12.0),
        Row(Modifier::new()
            .fill_max_width()
            .justify_content(JustifyContent::FLEX_END)
            .gap(Dp(8.0)))
        .child((
            TextButton(
                Modifier::new(),
                move || s_cancel.state.confirm_discard.set(None),
                Default::default(),
                || Text("Cancel").size(label),
            ),
            TextButton(
                Modifier::new(),
                move || {
                    s_drop.state.confirm_discard.set(None);
                    run_pending(&s_drop, pending_drop.clone());
                },
                Default::default(),
                || Text("Don't save").size(label),
            ),
            FilledTonalButton(
                Modifier::new(),
                move || {
                    s_save.dispatch_project(project_command_save(&s_save));
                    s_save.state.confirm_discard.set(None);
                    run_pending(&s_save, pending_save.clone());
                },
                Default::default(),
                || Text("Save").size(label),
            ),
        )),
    ));

    Dialog(
        state.clone(),
        store.overlay.clone(),
        Modifier::new(),
        DialogProperties {
            on_dismiss_request: Some(Rc::new({
                let store = store.clone();
                move || store.state.confirm_discard.set(None)
            })),
            ..Default::default()
        },
        content,
    )
}

/// Consume `last_error` into an M3 snackbar (renamite shell.rs pattern).
fn show_error_snackbar(store: &Store) {
    let controller = remember_with_key("snapshort_snackbar", {
        let overlay = store.overlay.clone();
        move || SnackbarController::new(overlay)
    });
    if let Some(message) = store.state.last_error.get() {
        store.state.last_error.set(None);
        store.state.status_msg.set(format!("Error: {message}"));
        let msg = message.clone();
        controller.show(SnackbarRequest {
            message,
            action: None,
            duration_ms: 4000,
            builder: Rc::new(move |dismissing| {
                Snackbar(
                    msg.clone(),
                    None,
                    Modifier::new(),
                    SnackbarConfig::default(),
                    dismissing,
                )
            }),
        });
    }
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

fn empty_overlay() -> View {
    // hit_passthrough + zero size so it never affects layout or input
    Box(Modifier::new()
        .width(Dp(0.0))
        .height(Dp(0.0))
        .hit_passthrough())
}

/// Blocking work (export) scrim with an M3 dialog-radius progress card.
fn loading_overlay(store: Rc<Store>) -> View {
    let Some(label) = store.state.blocking_operation.get() else {
        return empty_overlay();
    };

    let th = theme();

    Box(Modifier::new()
        .fill_max_size()
        .absolute()
        .offset(Some(Dp(0.0)), Some(Dp(0.0)), None, None)
        .background(Color(0, 0, 0, 140))
        .z_index(200.0))
    .child(
        Column(
            Modifier::new()
                .fill_max_size()
                .align_items(AlignItems::CENTER)
                .justify_content(repose_core::AlignContent::CENTER),
        )
        .child(Card(
            CardConfig {
                container_color: th.surface_container_high,
                shape_radius: Dp(28.0),
                ..Default::default()
            },
            move || {
                Column(
                    Modifier::new()
                        .padding(Dp(24.0))
                        .gap(Dp(12.0))
                        .align_items(AlignItems::CENTER),
                )
                .child((
                    CircularProgressIndicator(None, Default::default()),
                    Text(label).size(theme().typography.title_medium),
                    Text("Working…")
                        .size(theme().typography.body_medium)
                        .color(theme().on_surface_variant),
                ))
            },
        )),
    )
}
