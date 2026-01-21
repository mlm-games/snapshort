use super::{assets::assets_panel, timeline::timeline_panel};
use crate::state::Store;
use repose_core::{Color, Modifier, View};
use repose_ui::{Box, Column, Row, Text, TextStyle, ViewExt};
use snapshort_ui_core::{colors, icon_button};
use snapshort_usecases::PlaybackCommand;
use std::rc::Rc;

fn h_spacer(w: f32) -> View {
    Box(Modifier::new().width(w))
}
fn v_spacer(h: f32) -> View {
    Box(Modifier::new().height(h))
}

pub fn editor_screen(store: Rc<Store>) -> View {
    let main = Row(Modifier::new().flex_grow(1.0)).child((
        left_panel(store.clone()),
        Column(Modifier::new().flex_grow(1.0)).child((
            center_area(store.clone()),
            Row(Modifier::new().fill_max_width().height(350.0))
                .child(timeline_panel(store.clone())),
        )),
        right_panel(store.clone()),
    ));

    Column(Modifier::new().fill_max_size()).child((
        menu_bar(store.clone()),
        main,
        status_bar(store),
    ))
}

fn menu_bar(_store: Rc<Store>) -> View {
    Row(Modifier::new()
        .fill_max_width()
        .height(28.0)
        .background(colors::BG_PANEL)
        .padding_values(repose_core::PaddingValues {
            left: 8.0,
            right: 8.0,
            top: 0.0,
            bottom: 0.0,
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
        Text("Project Settings")
            .size(11.0)
            .color(colors::TEXT_MUTED),
    ])
}

fn menu_item(label: &str) -> View {
    Text(label)
        .size(12.0)
        .color(colors::TEXT_PRIMARY)
        .modifier(Modifier::new().padding(6.0).on_pointer_enter(|_| {}))
}

fn left_panel(store: Rc<Store>) -> View {
    Column(
        Modifier::new()
            .width(300.0)
            .fill_max_height()
            .background(colors::BG_PANEL)
            .border(1.0, colors::BORDER, 0.0),
    )
    .child((
        panel_header("Project"),
        Row(Modifier::new()
            .height(28.0)
            .background(colors::BG_PANEL)
            .border(1.0, colors::BORDER, 0.0)
            .padding(8.0))
        .child((
            panel_tab("Project", true),
            h_spacer(8.0),
            panel_tab("Media Browser", false),
            h_spacer(8.0),
            panel_tab("Effects", false),
        )),
        assets_panel(store),
        Box(Modifier::new()
            .fill_max_width()
            .height(1.0)
            .background(colors::BORDER)),
        Box(Modifier::new().fill_max_width().height(120.0).padding(8.0)).child((
            Text("Info Panel").size(12.0).color(colors::TEXT_MUTED),
            Text("No selection").size(11.0).color(colors::TEXT_DISABLED),
        )),
    ))
}

fn center_area(store: Rc<Store>) -> View {
    Column(
        Modifier::new()
            .fill_max_width()
            .flex_grow(1.0)
            .background(colors::BG_DARK),
    )
    .child((
        Column(
            Modifier::new()
                .fill_max_width()
                .flex_grow(1.0)
                .border(1.0, colors::BORDER, 0.0),
        )
        .child((
            panel_header("Program Monitor"),
            monitor_toolbar(store.clone()),
            Box(Modifier::new()
                .fill_max_width()
                .flex_grow(1.0)
                .padding(32.0)
                .background(Color::BLACK))
            .child(
                Box(Modifier::new()
                    .fill_max_size()
                    .background(colors::BG_DARK)
                    .border(1.0, colors::BORDER, 0.0))
                .child(Text("No Video").size(14.0).color(colors::TEXT_MUTED)),
            ),
            // Playback controls (wired)
            Row(Modifier::new()
                .height(40.0)
                .background(colors::BG_PANEL)
                .border(1.0, colors::BORDER, 0.0)
                .padding(8.0)
                .justify_content(repose_core::JustifyContent::Center))
            .child(vec![
                playback_button(
                    store.clone(),
                    "⏮",
                    PlaybackCommand::Seek {
                        frame: snapshort_domain::Frame(0),
                    },
                ),
                h_spacer(16.0),
                playback_seek_rel(store.clone(), "◀", -24),
                h_spacer(16.0),
                playback_button(store.clone(), "▶", PlaybackCommand::Play),
                h_spacer(16.0),
                playback_button(store.clone(), "⏸", PlaybackCommand::Pause),
                h_spacer(16.0),
                playback_button(store.clone(), "⏹", PlaybackCommand::Stop),
                h_spacer(16.0),
                playback_seek_rel(store.clone(), "⏭", 24),
            ]),
        )),
        Column(Modifier::new().fill_max_width().height(280.0)).child((
            panel_header("Source Monitor"),
            Box(Modifier::new()
                .fill_max_width()
                .flex_grow(1.0)
                .padding(16.0)
                .background(Color::BLACK))
            .child(
                Box(Modifier::new()
                    .fill_max_size()
                    .background(colors::BG_DARK)
                    .border(1.0, colors::BORDER, 0.0))
                .child(Text("Source").size(12.0).color(colors::TEXT_MUTED)),
            ),
        )),
    ))
}

fn right_panel(store: Rc<Store>) -> View {
    Column(
        Modifier::new()
            .width(280.0)
            .fill_max_height()
            .background(colors::BG_PANEL)
            .border(1.0, colors::BORDER, 0.0),
    )
    .child((
        panel_header("Inspector"),
        Row(Modifier::new()
            .height(28.0)
            .background(colors::BG_PANEL)
            .border(1.0, colors::BORDER, 0.0)
            .padding(8.0))
        .child((
            panel_tab("Effect Controls", true),
            h_spacer(8.0),
            panel_tab("Audio Clip Mixer", false),
            h_spacer(8.0),
            panel_tab("Metadata", false),
        )),
        inspector_content(store.clone()),
        Box(Modifier::new()
            .fill_max_width()
            .height(1.0)
            .background(colors::BORDER)),
        Column(Modifier::new().fill_max_width()).child((
            panel_header("History"),
            Box(Modifier::new().fill_max_width().height(200.0).padding(8.0)).child((
                Text("Project Created")
                    .size(11.0)
                    .color(colors::TEXT_PRIMARY),
                Text("Timeline Created")
                    .size(11.0)
                    .color(colors::TEXT_PRIMARY),
            )),
        )),
    ))
}

fn panel_header(title: &str) -> View {
    Row(Modifier::new()
        .fill_max_width()
        .height(28.0)
        .background(colors::BG_HEADER)
        .border(1.0, colors::BORDER, 0.0)
        .padding_values(repose_core::PaddingValues {
            left: 12.0,
            right: 8.0,
            top: 0.0,
            bottom: 0.0,
        })
        .align_items(repose_core::AlignItems::Center))
    .child((
        Text(title).size(12.0).color(colors::TEXT_HEADER),
        Box(Modifier::new().flex_grow(1.0)),
        Row(Modifier::new()).child((
            header_button("⁻"),
            h_spacer(4.0),
            header_button("□"),
            h_spacer(4.0),
            header_button("×"),
        )),
    ))
}

fn panel_tab(label: &str, active: bool) -> View {
    let bg = if active {
        colors::BG_ACTIVE_TAB
    } else {
        Color::TRANSPARENT
    };
    let text_color = if active {
        colors::TEXT_PRIMARY
    } else {
        colors::TEXT_MUTED
    };

    Box(Modifier::new()
        .background(bg)
        .padding_values(repose_core::PaddingValues {
            left: 8.0,
            right: 8.0,
            top: 4.0,
            bottom: 4.0,
        })
        .border(if active { 2.0 } else { 0.0 }, colors::ACCENT, 0.0))
    .child(Text(label).size(11.0).color(text_color))
}

fn monitor_toolbar(store: Rc<Store>) -> View {
    let playhead = store
        .state
        .timeline
        .get()
        .map(|t| t.playhead.0)
        .unwrap_or(0);

    Row(Modifier::new()
        .fill_max_width()
        .height(32.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(8.0)
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        Text("100%").size(11.0).color(colors::TEXT_PRIMARY),
        Box(Modifier::new().flex_grow(1.0)),
        Text("Full").size(11.0).color(colors::TEXT_MUTED),
        h_spacer(12.0),
        Box(Modifier::new()
            .width(1.0)
            .height(16.0)
            .background(colors::BORDER)),
        h_spacer(12.0),
        Text(format!("Frame: {playhead}"))
            .size(11.0)
            .color(colors::TEXT_PRIMARY),
        h_spacer(12.0),
        Box(Modifier::new()
            .width(1.0)
            .height(16.0)
            .background(colors::BORDER)),
        h_spacer(12.0),
        Text("Drop Frame").size(10.0).color(colors::TEXT_MUTED),
    ])
}

fn inspector_content(store: Rc<Store>) -> View {
    let selected_clip_id = store.state.selected_clip_id.get();
    let selected_asset_id = store.state.selected_asset_id.get();
    let timeline = store.state.timeline.get();
    let assets = store.state.assets.get();

    // Prefer clip selection if both exist
    if let (Some(clip_id), Some(tl)) = (selected_clip_id, timeline.clone()) {
        if let Some(clip) = tl.get_clip(clip_id) {
            let asset_name = clip
                .asset_id
                .and_then(|aid| assets.iter().find(|a| a.id == aid).map(|a| a.name.clone()))
                .unwrap_or_else(|| "-".into());

            return Column(Modifier::new().fill_max_width().flex_grow(1.0).padding(8.0)).child((
                Text("Selected Clip").size(12.0).color(colors::TEXT_PRIMARY),
                v_spacer(8.0),
                kv("Clip ID", format!("{}", clip.id.0)),
                kv("Asset", asset_name),
                kv(
                    "Track",
                    format!(
                        "{}{}",
                        match clip.track.track_type {
                            snapshort_domain::TrackType::Video => "V",
                            snapshort_domain::TrackType::Audio => "A",
                        },
                        clip.track.index + 1
                    ),
                ),
                kv("Start (frame)", format!("{}", clip.timeline_start.0)),
                kv("End (frame)", format!("{}", clip.timeline_end().0)),
                kv(
                    "Duration (frames)",
                    format!("{}", clip.effective_duration()),
                ),
            ));
        }
    }

    if let Some(asset_id) = selected_asset_id {
        if let Some(a) = assets.iter().find(|a| a.id == asset_id) {
            let path = a.path.to_string_lossy().to_string();
            let status = format!("{:?}", a.status);
            let dur = a.media_info.as_ref().map(|m| m.duration_ms).unwrap_or(0);

            return Column(Modifier::new().fill_max_width().flex_grow(1.0).padding(8.0)).child((
                Text("Selected Asset")
                    .size(12.0)
                    .color(colors::TEXT_PRIMARY),
                v_spacer(8.0),
                kv("Name", a.name.clone()),
                kv("Type", format!("{:?}", a.asset_type)),
                kv("Status", status),
                kv("Duration (ms)", format!("{dur}")),
                kv("Path", path),
            ));
        }
    }

    Column(Modifier::new().fill_max_width().flex_grow(1.0).padding(8.0)).child((
        Text("Inspector").size(12.0).color(colors::TEXT_MUTED),
        v_spacer(6.0),
        Text("Click an asset or a clip to inspect it.")
            .size(11.0)
            .color(colors::TEXT_DISABLED),
    ))
}

fn kv(label: impl Into<String>, value: impl Into<String>) -> View {
    Row(Modifier::new()
        .fill_max_width()
        .height(22.0)
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        Text(label.into()).size(11.0).color(colors::TEXT_MUTED),
        Box(Modifier::new().flex_grow(1.0)),
        Text(value.into()).size(11.0).color(colors::TEXT_PRIMARY),
    ])
}

fn header_button(icon: &str) -> View {
    Box(Modifier::new()
        .width(20.0)
        .height(20.0)
        .padding(2.0)
        .on_pointer_enter(|_| {}))
    .child(Text(icon).size(12.0).color(colors::TEXT_MUTED))
}

fn playback_button(store: Rc<Store>, icon: &str, cmd: PlaybackCommand) -> View {
    icon_button(icon, move || store.dispatch_playback(cmd.clone()))
        .modifier(Modifier::new().width(32.0).height(32.0))
}

fn playback_seek_rel(store: Rc<Store>, icon: &str, delta: i64) -> View {
    icon_button(icon, move || {
        let cur = store
            .state
            .timeline
            .get()
            .map(|t| t.playhead.0)
            .unwrap_or(0);
        store.dispatch_playback(PlaybackCommand::Seek {
            frame: snapshort_domain::Frame((cur + delta).max(0)),
        });
    })
    .modifier(Modifier::new().width(32.0).height(32.0))
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
        .height(24.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding_values(repose_core::PaddingValues {
            left: 8.0,
            right: 8.0,
            top: 0.0,
            bottom: 0.0,
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
