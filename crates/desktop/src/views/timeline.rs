use super::dnd::{as_drag_payload, AssetDragPayload, ClipDragPayload, TrimPayload};
use crate::state::Store;
use miniter_domain::{Clip, ClipId, ClipKind, MediaDuration, Timestamp, Track, TrackId, TrackKind, Transition, TransitionKind};
use miniter_usecases::EditCommand;
use snapshort_ui_core::Icons;
use repose_core::{
    dnd::{DragOver, DragPayload, DragStart, DropEvent},
    view::View,
    Color, CursorIcon, Modifier, Vec2,
};
use repose_material::Icon;
use repose_ui::{
    scroll::{remember_scroll_state, remember_scroll_state_xy, ScrollArea, ScrollAreaXY},
    Box, Button, Column, Image, ImageExt, Row, Slider, Stack, Text, TextStyle, ViewExt,
};
use snapshort_ui_core::{audio_waveform, colors};
use snapshort_usecases::{AssetType, PlaybackCommand, PreviewCommand};
use std::rc::Rc;

const MICROS_PER_FRAME: i64 = 1_000_000 / 30; // approximate 30fps frame in microseconds

fn h_spacer(w: f32) -> View {
    Box(Modifier::new().width(w))
}

fn track_kind_color(kind: TrackKind) -> Color {
    match kind {
        TrackKind::Video => colors::VIDEO_TRACK,
        TrackKind::Audio => colors::AUDIO_TRACK,
        _ => colors::TEXT_MUTED,
    }
}

fn track_kind_bg(kind: TrackKind) -> Color {
    match kind {
        TrackKind::Video => Color::from_rgb(0x1E, 0x3A, 0x5F),
        TrackKind::Audio => Color::from_rgb(0x2D, 0x5A, 0x27),
        _ => Color::from_rgb(0x1E, 0x1E, 0x24),
    }
}

fn track_kind_label(kind: TrackKind, index: usize) -> String {
    match kind {
        TrackKind::Video => format!("V{}", index + 1),
        TrackKind::Audio => format!("A{}", index + 1),
        _ => format!("?{}", index + 1),
    }
}

fn track_row_height(kind: TrackKind) -> f32 {
    match kind {
        TrackKind::Video => 64.0,
        TrackKind::Audio => 52.0,
        _ => 52.0,
    }
}

fn clip_row_height(kind: TrackKind) -> f32 {
    match kind {
        TrackKind::Video => 52.0,
        TrackKind::Audio => 40.0,
        _ => 40.0,
    }
}

fn us_to_seconds(us: i64) -> f64 {
    us as f64 / 1_000_000.0
}

fn timecode_from_us(us: i64, fps: f64) -> String {
    if fps <= 0.0 {
        return "00:00:00:00".to_string();
    }
    let total_secs = us as f64 / 1_000_000.0;
    let total_frames = (total_secs * fps).round() as i64;
    let fps_int = fps.round() as i64;
    let secs = total_frames / fps_int;
    let rem_frames = total_frames % fps_int;
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let secs_remain = secs % 60;
    format!("{:02}:{:02}:{:02}:{:02}", hours, mins, secs_remain, rem_frames)
}

fn duration_to_seconds(d: MediaDuration) -> f64 {
    d.as_micros() as f64 / 1_000_000.0
}

pub fn timeline_panel(store: Rc<Store>) -> View {
    let timeline = store.state.timeline.get();

    let name = "Timeline"; // miniter timelime has no name field

    let total_us = timeline
        .as_ref()
        .map(|t| t.duration_end().as_micros())
        .unwrap_or(0);
    let fps = 30.0; // default fps
    let timecode = timecode_from_us(total_us, fps);

    let playhead_us = store.state.playhead.get().0;
    let playhead_tc = timecode_from_us(playhead_us, fps);

    let px_per_micro = store.state.timeline_zoom.get() / 1_000_000.0;
    let px_per_sec = px_per_micro * 1_000_000.0;
    let track_header_scroll_state = remember_scroll_state("timeline_headers_y");
    let track_scroll_xy_state = remember_scroll_state_xy("timeline_tracks_xy");
    let (mut scroll_x, track_scroll_y) = track_scroll_xy_state.get();
    track_header_scroll_state.set_offset(track_scroll_y);

    if store.state.playback_state.get() == "Playing" && timeline.is_some() {
        let playhead_px = playhead_us as f32 * px_per_micro;
        let vp_w_est = 700.0;
        let margin = vp_w_est * 0.33;
        let target = playhead_px - margin;
        if target > scroll_x + 20.0 || target < scroll_x - vp_w_est * 0.5 {
            scroll_x = target.max(0.0);
            track_scroll_xy_state.set_offset_xy(scroll_x, track_scroll_y);
        }
    }

    let store_for_split = store.clone();

    let tracks: Vec<&Track> = timeline
        .as_ref()
        .map(|tl| tl.tracks.iter().collect())
        .unwrap_or_default();

    let mut track_header_views: Vec<View> = Vec::new();
    track_header_views.push(Box(Modifier::new()
        .fill_max_width()
        .height(24.0)
        .background(colors::BG_PANEL)));

    for (idx, track) in tracks.iter().enumerate() {
        let key = idx as u64;
        let kind = track.kind;
        track_header_views.push(track_header(store.clone(), &track.name, kind, Some(track), key));
    }
    if tracks.is_empty() {
        track_header_views.push(track_header(store.clone(), "V1", TrackKind::Video, None, 0));
        track_header_views.push(track_header(store.clone(), "A1", TrackKind::Audio, None, 1000));
    }

    track_header_views.push(track_add_buttons(store.clone()));

    let mut track_content_views: Vec<View> = Vec::new();
    track_content_views.push(time_ruler(
        store.clone(),
        track_scroll_xy_state.clone(),
        px_per_sec,
    ));

    if !tracks.is_empty() {
        for (idx, track) in tracks.iter().enumerate() {
            track_content_views.push(track_lane(
                store.clone(),
                track,
                idx,
                px_per_micro,
                track_scroll_xy_state.clone(),
            ));
        }
    } else {
        track_content_views.push(empty_lane(TrackKind::Video));
        track_content_views.push(empty_lane(TrackKind::Audio));
    }

    let info = timeline
        .as_ref()
        .map(|_| "Timeline".to_string())
        .unwrap_or_else(|| "-".to_string());

    let header_left = Row(Modifier::new().align_items(repose_core::AlignItems::Center)).child((
        Text(name)
            .size(12.0)
            .color(colors::TEXT_PRIMARY)
            .single_line(),
        h_spacer(8.0),
        Text(info)
            .size(10.0)
            .color(colors::TEXT_MUTED)
            .single_line(),
    ));

    let header_tools = Row(Modifier::new().align_items(repose_core::AlignItems::Center)).child((
        tool_group(vec![
            tool_icon_button(Icons::content_cut, {
                let store = store_for_split.clone();
                move || {
                    if let (Some(clip_id), Some(tl)) = (
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
                        markers.push(crate::state::TimelineMarker { timestamp_us: at, label });
                        store.state.timeline_markers.set(markers);
                    }
                }
            }),
            tool_icon_button(Icons::transition, {
                let store = store.clone();
                move || {
                    let clip_id = store.state.selected_clip_id.get();
                    let Some(id) = clip_id else { return };
                    let timeline = store.state.timeline.get();
                    let Some(tl) = timeline else { return };
                    let has_transition = tl.tracks.iter()
                        .flat_map(|t| &t.clips)
                        .any(|c| c.id == id && c.transition_in.is_some());
                    let transition = if has_transition {
                        None
                    } else {
                        Some(Transition::new(
                            TransitionKind::CrossFade,
                            MediaDuration::from_micros(500_000),
                        ))
                    };
                    store.dispatch_edit(EditCommand::SetTransitionIn {
                        clip_id: id,
                        transition,
                    });
                }
            }),
            snap_toggle(store.clone()),
        ]),
        h_spacer(10.0),
        tool_group(vec![
            Text("Zoom").size(10.0).color(colors::TEXT_MUTED),
            h_spacer(6.0),
            Slider(store.state.timeline_zoom.get(), (0.5, 12.0), None, {
                let store = store.clone();
                move |value| store.state.timeline_zoom.set(value)
            })
            .modifier(Modifier::new().width(90.0).height(18.0)),
        ]),
    ));

    let header_timecode = tool_group(vec![
        Text(playhead_tc)
            .size(11.0)
            .color(colors::TEXT_ACCENT)
            .single_line(),
        h_spacer(4.0),
        Text("/").size(11.0).color(colors::TEXT_MUTED),
        h_spacer(4.0),
        Text(timecode)
            .size(11.0)
            .color(colors::TEXT_PRIMARY)
            .single_line(),
    ]);

    let header = Row(Modifier::new()
        .fill_max_width()
        .height(34.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding_values(repose_core::PaddingValues {
            left: 10.0,
            right: 10.0,
            top: 4.0,
            bottom: 4.0,
        })
        .align_items(repose_core::AlignItems::Center))
    .child((
        header_left,
        Box(Modifier::new().flex_grow(1.0)),
        header_tools,
        Box(Modifier::new().flex_grow(1.0)),
        header_timecode,
    ));

    let content = Row(Modifier::new().fill_max_size().flex_grow(1.0)).child((
        ScrollArea(
            Modifier::new().width(200.0).fill_max_height(),
            track_header_scroll_state.clone(),
            Column(Modifier::new().fill_max_width().min_width(200.0).border(
                1.0,
                colors::BORDER,
                0.0,
            ))
            .child(track_header_views),
        ),
        Column(Modifier::new().fill_max_width().flex_grow(1.0)).child((Stack(
            Modifier::new().fill_max_size(),
        )
        .child((
            ScrollAreaXY(
                Modifier::new().fill_max_size(),
                track_scroll_xy_state.clone(),
                Column(Modifier::new().fill_max_width().min_width(1200.0))
                    .child(track_content_views),
            ),
            playhead_at_scroll(playhead_us, px_per_micro, track_scroll_xy_state, {
                let store = store.clone();
                move |us| {
                    store.dispatch_playback(PlaybackCommand::Seek {
                        timestamp: Timestamp(us.max(0)),
                    });
                }
            }),
        )),)),
    ));

    Column(
        Modifier::new()
            .fill_max_size()
            .height(350.0)
            .background(colors::BG_DARK),
    )
    .child((header, content))
}

fn track_header(store: Rc<Store>, name: &str, kind: TrackKind, track: Option<&Track>, key: u64) -> View {
    let h = track_row_height(kind);
    Row(Modifier::new()
        .key(key)
        .fill_max_width()
        .height(h)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(4.0)
        .align_items(repose_core::AlignItems::Center))
    .child((
        Box(Modifier::new()
            .width(10.0)
            .height(10.0)
            .border(2.0, track_kind_color(kind), 0.0)),
        Box(Modifier::new().width(4.0)),
        Text(name)
            .size(10.0)
            .color(colors::TEXT_PRIMARY)
            .single_line()
            .modifier(Modifier::new().flex_grow(1.0)),
        if let Some(track) = track {
            let track_id = track.id;
            let muted = track.muted;
            let locked = track.locked;
            Row(Modifier::new().align_items(repose_core::AlignItems::Center).gap(2.0)).child((
                icon_btn(if muted { Icons::volume_off } else { Icons::volume_up }, 14.0, {
                    let store = store.clone();
                    move || store.dispatch_edit(EditCommand::SetTrackMuted { track_id, muted: !muted })
                }),
                icon_btn(if locked { Icons::lock } else { Icons::lock_open }, 14.0, {
                    let store = store.clone();
                    move || store.dispatch_edit(EditCommand::SetTrackLocked { track_id, locked: !locked })
                }),
                icon_btn(Icons::delete, 14.0, {
                    let store = store.clone();
                    move || store.dispatch_edit(EditCommand::RemoveTrack { track_id })
                }),
            ))
        } else {
            Box(Modifier::new().width(1.0).height(1.0))
        },
    ))
}

fn icon_btn(icon: repose_material::Symbol, size: f32, on_click: impl Fn() + 'static) -> View {
    Box(Modifier::new()
        .size(size + 6.0, size + 6.0)
        .clickable()
        .on_pointer_down(move |_| on_click())
        .align_items(repose_core::AlignItems::Center)
        .justify_content(repose_core::JustifyContent::Center))
    .child(Icon(icon).size(size).color(colors::TEXT_MUTED))
}

fn track_add_buttons(store: Rc<Store>) -> View {
    let v_count = store
        .state
        .timeline
        .get()
        .as_ref()
        .map(|tl| tl.tracks.iter().filter(|t| t.kind == TrackKind::Video).count())
        .unwrap_or(0)
        + 1;
    let a_count = store
        .state
        .timeline
        .get()
        .as_ref()
        .map(|tl| tl.tracks.iter().filter(|t| t.kind == TrackKind::Audio).count())
        .unwrap_or(0)
        + 1;

    Row(Modifier::new()
        .fill_max_width()
        .height(44.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(6.0)
        .align_items(repose_core::AlignItems::Center))
    .child((
        {
            let store = store.clone();
            Box(Modifier::new()
                .height(28.0)
                .min_width(32.0)
                .clip_rounded(14.0)
                .padding_values(repose_core::PaddingValues { left: 8.0, right: 8.0, top: 0.0, bottom: 0.0 })
                .align_items(repose_core::AlignItems::Center)
                .justify_content(repose_core::JustifyContent::Center)
                .clickable()
                .on_pointer_down(move |_| store.dispatch_edit(EditCommand::AddTrack {
                    kind: TrackKind::Video,
                    name: format!("V{v_count}"),
                })))
            .child(Text("+V").color(colors::TEXT_ACCENT).size(12.0).single_line())
        },
        h_spacer(8.0),
        {
            let store = store.clone();
            Box(Modifier::new()
                .height(28.0)
                .min_width(32.0)
                .clip_rounded(14.0)
                .padding_values(repose_core::PaddingValues { left: 8.0, right: 8.0, top: 0.0, bottom: 0.0 })
                .align_items(repose_core::AlignItems::Center)
                .justify_content(repose_core::JustifyContent::Center)
                .clickable()
                .on_pointer_down(move |_| store.dispatch_edit(EditCommand::AddTrack {
                    kind: TrackKind::Audio,
                    name: format!("A{a_count}"),
                })))
            .child(Text("+A").color(colors::TEXT_ACCENT).size(12.0).single_line())
        },
    ))
}

fn snap_toggle(store: Rc<Store>) -> View {
    let enabled = store.state.timeline_snap.get();
    let th = repose_core::theme();
    let (bg, fg) = if enabled {
        (th.primary_container, th.on_primary_container)
    } else {
        (th.surface_variant, th.on_surface_variant)
    };
    Box(Modifier::new()
        .height(28.0)
        .min_width(48.0)
        .background(bg)
        .clip_rounded(14.0)
        .padding_values(repose_core::PaddingValues {
            left: 12.0,
            right: 12.0,
            top: 0.0,
            bottom: 0.0,
        })
        .align_items(repose_core::AlignItems::Center)
        .justify_content(repose_core::JustifyContent::Center)
        .clickable()
        .on_pointer_down(move |_| {
            let current = store.state.timeline_snap.get();
            store.state.timeline_snap.set(!current);
        }))
    .child(Text("Snap").size(12.0).color(fg).single_line())
}

fn tool_group(children: Vec<View>) -> View {
    let th = repose_core::theme();
    Row(Modifier::new()
        .align_items(repose_core::AlignItems::Center)
        .padding_values(repose_core::PaddingValues {
            left: 4.0,
            right: 4.0,
            top: 3.0,
            bottom: 3.0,
        })
        .background(th.surface_container)
        .clip_rounded(14.0))
    .child(children)
}

fn tool_icon_button(icon: repose_material::Symbol, on_click: impl Fn() + 'static) -> View {
    let th = repose_core::theme();
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
        .align_items(repose_core::AlignItems::Center)
        .justify_content(repose_core::JustifyContent::Center)
        .clickable()
        .on_pointer_down(move |_| on_click()))
    .child(Icon(icon).color(th.primary).size(16.0))
}

fn time_ruler(
    store: Rc<Store>,
    scroll_state_xy: std::rc::Rc<repose_ui::scroll::ScrollStateXY>,
    px_per_sec: f32,
) -> View {
    // Pick a time interval so labels are at least ~60px apart
    let min_label_px = 60.0;
    let raw_interval_secs = (min_label_px / px_per_sec.max(0.001)) as i64;
    let interval_secs = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1800, 3600]
        .into_iter()
        .find(|i| *i >= raw_interval_secs)
        .unwrap_or(3600);
    let interval_px = px_per_sec * interval_secs as f32;

    let total_px = 2000.0;
    let marker_count = (total_px / interval_px).ceil() as i64;

    let mut tick_views: Vec<View> = Vec::new();
    let mut sec = 0i64;
    for _ in 0..marker_count.min(200) {
        let tc = if interval_secs >= 60 {
            format!("{:02}:{:02}", sec / 60, sec % 60)
        } else {
            format!("{}s", sec)
        };
        tick_views.push(time_marker(&tc, interval_px));
        sec += interval_secs;
    }

    let user_markers = store.state.timeline_markers.get();
    for m in &user_markers {
        let x = (m.timestamp_us as f32 / 1_000_000.0) * px_per_sec;
        tick_views.push(
            Box(Modifier::new()
                .absolute()
                .offset(Some(x - 4.0), Some(0.0), None, None)
                .width(8.0)
                .height(8.0)
                .background(colors::MARKER)
                .z_index(10.0)),
        );
    }

    Row(Modifier::new()
        .fill_max_width()
        .height(24.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding_values(repose_core::PaddingValues {
            left: 0.0,
            right: 0.0,
            top: 2.0,
            bottom: 2.0,
        })
        .align_items(repose_core::AlignItems::End)
        .on_pointer_down({
            let store = store.clone();
            let scroll_state_xy = scroll_state_xy.clone();
            move |event| {
                let (scroll_x, _scroll_y) = scroll_state_xy.get();
                let us = ((event.position.x + scroll_x) / px_per_sec.max(0.001) * 1_000_000.0) as i64;
                store.dispatch_playback(PlaybackCommand::Seek {
                    timestamp: Timestamp(us.max(0)),
                });
            }
        })
        .on_scroll({
            let store = store.clone();
            move |delta| {
                if delta.y.abs() > delta.x.abs() {
                    let factor = if delta.y > 0.0 { 1.1 } else { 1.0 / 1.1 };
                    let current = store.state.timeline_zoom.get();
                    store.state.timeline_zoom.set((current * factor).clamp(0.5, 20.0));
                }
                Vec2::default()
            }
        }))
    .child(tick_views)
}

fn time_marker(label: &str, width: f32) -> View {
    Box(Modifier::new().width(width).height(20.0)).child(
        Column(Modifier::new().align_items(repose_core::AlignItems::Start)).child((
            Box(Modifier::new()
                .width(1.0)
                .height(6.0)
                .background(colors::TEXT_MUTED)),
            Text(label).size(9.0).color(colors::TEXT_MUTED),
        )),
    )
}

fn empty_lane(kind: TrackKind) -> View {
    Row(Modifier::new()
        .fill_max_width()
        .height(track_row_height(kind))
        .background(colors::BG_TRACK)
        .border(1.0, colors::BORDER, 0.0)
        .padding(4.0))
    .child((
        Box(Modifier::new().flex_grow(1.0)),
        Text(match kind {
            TrackKind::Video => "No clips (V)",
            TrackKind::Audio => "No clips (A)",
            _ => "No clips",
        })
        .size(10.0)
        .color(colors::TEXT_DISABLED),
        Box(Modifier::new().flex_grow(1.0)),
    ))
}

fn track_lane(
    store: Rc<Store>,
    track: &Track,
    _track_index: usize,
    px_per_micro: f32,
    scroll_state_xy: std::rc::Rc<repose_ui::scroll::ScrollStateXY>,
) -> View {
    let track_id = track.id;
    let kind = track.kind;

    let mut clips: Vec<&Clip> = track.clips.iter().collect();
    clips.sort_by_key(|c| c.timeline_start.0);
    let track_min_start = clips
        .iter()
        .map(|c| c.timeline_start.0)
        .min()
        .unwrap_or(0)
        .max(0);

    let selected_clip = store.state.selected_clip_id.get();

    let mut children: Vec<View> = Vec::new();
    let mut cursor: i64 = track_min_start;

    for clip in clips.iter() {
        let start = clip.timeline_start.0;
        let gap_us = (start - cursor).max(0);
        if gap_us > 0 {
            children.push(Box(Modifier::new().width(gap_us as f32 * px_per_micro)));
        }
        children.push(clip_view(
            store.clone(),
            clip,
            kind,
            px_per_micro,
            selected_clip,
            track_id,
        ));
        cursor = clip.timeline_end().as_micros();
    }

    children.push(Box(Modifier::new().flex_grow(1.0)));

    let store_for_drop = store.clone();
    let bg = if store.state.drag_hover_track.get() == Some(track_id) {
        colors::BG_HOVER
    } else {
        colors::BG_TRACK
    };

    Row(Modifier::new()
        .fill_max_width()
        .height(track_row_height(kind))
        .background(bg)
        .border(1.0, colors::BORDER, 0.0)
        .padding(4.0)
        .align_items(repose_core::AlignItems::Center)
        .on_pointer_down({
            let store = store.clone();
            let scroll_state_xy = scroll_state_xy.clone();
            move |event| {
                let (scroll_x, _scroll_y) = scroll_state_xy.get();
                let us = ((event.position.x + scroll_x) / px_per_micro.max(0.0001)) as i64;
                store.dispatch_playback(PlaybackCommand::Seek {
                    timestamp: Timestamp(us.max(0)),
                });
                store.state.selected_clip_id.set(None);
                store.state.selected_asset_id.set(None);
            }
        })
        .on_drag_enter({
            let store = store.clone();
            move |_| {
                store.state.drag_hover_track.set(Some(track_id));
            }
        })
        .on_drag_leave({
            let store = store.clone();
            move |_| {
                store.state.drag_hover_track.set(None);
            }
        })
        .on_drag_over({
            move |_: DragOver| {}
        })
        .on_drop({
            let store = store.clone();
            let track_id = track_id;
            let scroll_state_xy = scroll_state_xy.clone();
            move |event: DropEvent| {
                store.state.drag_hover_track.set(None);
                let (scroll_x, _scroll_y) = scroll_state_xy.get();
                let drop_us =
                    ((event.position.x + scroll_x) / px_per_micro.max(0.0001)) as i64;
                if let Some(payload) = event.payload.downcast_ref::<TrimPayload>() {
                    let timeline = store_for_drop.state.timeline.get();
                    let Some(timeline) = timeline else {
                        return false;
                    };
                    let found = timeline.tracks.iter().find_map(|t| t.clip_by_id(payload.clip_id));
                    let Some(clip) = found else {
                        return false;
                    };

                    if payload.is_start {
                        if drop_us <= clip.timeline_start.0
                            || drop_us >= clip.timeline_end().as_micros()
                        {
                            return true;
                        }
                        let new_start = Timestamp(drop_us.max(0));
                        let delta = clip.timeline_start - new_start;
                        let new_source_start = clip.source_start + delta;
                        store_for_drop.dispatch_edit(EditCommand::TrimClipStart {
                            clip_id: payload.clip_id,
                            new_start,
                            new_source_start,
                        });
                    } else {
                        if drop_us <= clip.timeline_start.0 {
                            return true;
                        }
                        let new_duration = MediaDuration::from_micros(
                            (drop_us - clip.timeline_start.0).max(1),
                        );
                        store_for_drop.dispatch_edit(EditCommand::TrimClipEnd {
                            clip_id: payload.clip_id,
                            new_duration,
                        });
                    }
                    return true;
                }
                if let Some(payload) = event.payload.downcast_ref::<ClipDragPayload>() {
                    if payload.original_start.0 == drop_us.max(0)
                        && payload.original_track == track_id
                    {
                        return true;
                    }
                    store_for_drop.dispatch_edit(EditCommand::MoveClip {
                        clip_id: payload.clip_id,
                        new_track_id: track_id,
                        new_start: Timestamp(drop_us.max(0)),
                    });
                    return true;
                }
                if let Some(payload) = event.payload.downcast_ref::<AssetDragPayload>() {
                    let assets = store_for_drop.state.assets.get();
                    let Some(asset) = assets.iter().find(|a| a.id == payload.asset_id) else {
                        return false;
                    };

                    if !asset.status.is_usable() {
                        store_for_drop
                            .state
                            .status_msg
                            .set(format!("{} is not ready yet", asset.name));
                        return false;
                    }

                    let allowed = match kind {
                        TrackKind::Video => matches!(
                            asset.asset_type,
                            AssetType::Video | AssetType::Image | AssetType::Sequence
                        ),
                        TrackKind::Audio => matches!(asset.asset_type, AssetType::Audio),
                        _ => false,
                    };

                    if !allowed {
                        return false;
                    }

                    let duration_us = asset
                        .media_info
                        .as_ref()
                        .map(|m| (m.duration_ms as i64 * 1000).max(1))
                        .unwrap_or(1_000_000);

                    let source_path = asset.effective_path().to_string_lossy().to_string();
                    let (width, height, fps) = asset
                        .media_info
                        .as_ref()
                        .and_then(|m| {
                            m.primary_video()
                                .map(|v| (v.width, v.height, v.fps))
                        })
                        .unwrap_or((1920, 1080, 30.0));

                    let clip_kind = match kind {
                        TrackKind::Audio => ClipKind::Audio(miniter_domain::AudioClip {
                            source_path,
                            sample_rate: 48000,
                            channels: 2,
                            filters: vec![],
                        }),
                        _ => ClipKind::Video(miniter_domain::VideoClip {
                            source_path,
                            width,
                            height,
                            fps,
                            filters: vec![],
                            audio_filters: vec![],
                        }),
                    };

                    let clip = Clip {
                        id: ClipId::new(),
                        timeline_start: Timestamp(drop_us.max(0)),
                        timeline_duration: MediaDuration::from_micros(duration_us),
                        source_start: MediaDuration::ZERO,
                        source_end: MediaDuration::from_micros(duration_us),
                        source_total_duration: MediaDuration::from_micros(duration_us),
                        speed: 1.0,
                        volume: 1.0,
                        opacity: 1.0,
                        muted: false,
                        transition_in: None,
                        transition_out: None,
                        kind: clip_kind,
                        keyframes: Default::default(),
                    };

                    store_for_drop.dispatch_edit(EditCommand::AddClip {
                        track_id,
                        clip,
                    });
                    return true;
                }
                false
            }
        }))
    .child(children)
}

fn seek_at_us(store: &Store, px_per_micro: f32, x: f32) {
    if px_per_micro <= 0.0 {
        return;
    }
    let us = (x / px_per_micro) as i64;
    store.dispatch_playback(PlaybackCommand::Seek {
        timestamp: Timestamp(us.max(0)),
    });
}

fn us_from_x(store: &Store, px_per_micro: f32, x: f32) -> i64 {
    if px_per_micro <= 0.0 {
        return 0;
    }
    let raw = (x / px_per_micro) as i64;
    if !store.state.timeline_snap.get() {
        return raw;
    }

    let mut candidates: Vec<i64> = vec![0];
    let timeline = store.state.timeline.get();
    let Some(tl) = timeline else {
        return raw;
    };

    for track in &tl.tracks {
        for clip in &track.clips {
            candidates.push(clip.timeline_start.0);
            candidates.push(clip.timeline_end().as_micros());
        }
    }

    let snap_threshold = 8.0_f32;
    let mut best = raw;
    let mut best_dist = snap_threshold + 1.0;
    for c in candidates {
        let dist = ((c - raw).abs() as f32) * px_per_micro;
        if dist <= snap_threshold && dist < best_dist {
            best = c;
            best_dist = dist;
        }
    }

    best
}

fn playhead_at_scroll(
    playhead_us: i64,
    px_per_micro: f32,
    scroll_state_xy: std::rc::Rc<repose_ui::scroll::ScrollStateXY>,
    on_seek: impl Fn(i64) + 'static,
) -> View {
    let (scroll_x, _scroll_y) = scroll_state_xy.get();
    let x = playhead_us as f32 * px_per_micro - scroll_x;
    let line_color = colors::ACCENT;
    let seek_px = px_per_micro;
    let seek_scroll = scroll_state_xy.clone();

    repose_canvas::Canvas(
        Modifier::new().fill_max_height().width(12.0),
        move |scope: &mut repose_canvas::DrawScope| {
            let height = scope.size.height;
            let width = scope.size.width;

            scope.draw_rect_stroke(
                repose_core::Rect {
                    x: width / 2.0 - 0.5,
                    y: 0.0,
                    w: 1.0,
                    h: height,
                },
                line_color,
                0.0,
                1.0,
            );

            scope.draw_circle(
                repose_core::Vec2 {
                    x: width / 2.0,
                    y: 6.0,
                },
                5.0,
                line_color,
            );
        },
    )
    .modifier(
        Modifier::new()
            .absolute()
            .offset(Some(x - 6.0), Some(0.0), None, None)
            .z_index(100.0)
            .clickable()
            .on_pointer_down(move |event| {
                let (scroll_x, _scroll_y) = seek_scroll.get();
                let us = ((event.position.x + scroll_x) / seek_px.max(0.0001)) as i64;
                on_seek(us.max(0));
            }),
    )
}

fn transition_indicator(render_w: f32, clip_h: f32, is_in: bool) -> View {
    let x = if is_in { 0.0 } else { render_w - 4.0 };
    Box(Modifier::new()
        .width(4.0)
        .height(clip_h * 0.5)
        .background(colors::ACCENT)
        .absolute()
        .offset(Some(x), Some(clip_h * 0.25), None, None)
        .z_index(10.0)
        )
}

fn clip_view(
    store: Rc<Store>,
    clip: &Clip,
    kind: TrackKind,
    px_per_micro: f32,
    selected_clip: Option<ClipId>,
    track_id: TrackId,
) -> View {
    let dur_us = clip.timeline_duration.as_micros().max(1);
    let render_w = (dur_us as f32 * px_per_micro).max(1.0);

    let (bg, border, label) = {
        let clip_name = match &clip.kind {
            ClipKind::Video(_) => "Video",
            ClipKind::Audio(_) => "Audio",
            ClipKind::Text(t) => &t.text,
            ClipKind::Subtitle(_) => "Subtitle",
            _ => "Clip",
        };
        (
            track_kind_bg(kind),
            track_kind_color(kind),
            clip_name.to_string(),
        )
    };

    let is_selected = selected_clip == Some(clip.id);
    let border_color = if is_selected { colors::ACCENT } else { border };
    let bg_color = if is_selected { colors::BG_SELECTED } else { bg };

    let clip_id = clip.id;
    let original_start = clip.timeline_start;
    let original_track = track_id;

    let clip_h = clip_row_height(kind);
    let show_details = render_w >= 64.0;

    let waveform = if kind == TrackKind::Audio && show_details {
        let waveform_width = (render_w - 24.0).max(10.0);
        let waveform_height = (clip_h - 18.0).clamp(8.0, 18.0);
        let _assets = store.state.assets.get();
        let wave_data: Option<&[f32]> = match &clip.kind {
            ClipKind::Audio(a) => {
                let sp = a.source_path.as_str();
                _assets
                    .iter()
                    .find(|a| a.effective_path().to_string_lossy().as_ref() == sp)
                    .and_then(|a| a.media_info.as_ref())
                    .and_then(|m| m.waveform.as_deref())
            }
            _ => None,
        };
        audio_waveform(
            waveform_width,
            waveform_height,
            wave_data,
            colors::AUDIO_TRACK,
        )
    } else if kind != TrackKind::Audio && show_details {
        clip_thumbnails(store.clone(), clip, render_w)
    } else {
        Box(Modifier::new().width(1.0).height(1.0))
    };
    let label_view: View = if show_details {
        Text(label).size(10.0).color(colors::TEXT_PRIMARY)
    } else {
        Box(Modifier::new().width(1.0).height(1.0))
    };

    let left_handle = Box(Modifier::new()
        .width(6.0)
        .fill_max_height()
        .background(if is_selected {
            colors::ACCENT
        } else {
            colors::TRANSPARENT
        })
        .cursor(CursorIcon::EwResize)
        .on_drag_start({
            let clip_id = clip_id;
            move |_: DragStart| -> Option<DragPayload> {
                Some(as_drag_payload(TrimPayload {
                    clip_id,
                    is_start: true,
                }))
            }
        })
        .on_drag_end(move |_| {}));

    let right_handle = Box(Modifier::new()
        .width(6.0)
        .fill_max_height()
        .background(if is_selected {
            colors::ACCENT
        } else {
            colors::TRANSPARENT
        })
        .cursor(CursorIcon::EwResize)
        .on_drag_start({
            let clip_id = clip_id;
            move |_: DragStart| -> Option<DragPayload> {
                Some(as_drag_payload(TrimPayload {
                    clip_id,
                    is_start: false,
                }))
            }
        })
        .on_drag_end(move |_| {}));

    let clip_content = Row(Modifier::new()
        .width(render_w)
        .height(clip_h)
        .background(bg_color)
        .border(1.0, border_color, 2.0)
        .cursor(CursorIcon::Grab)
        .on_drag_start({
            move |_: DragStart| -> Option<DragPayload> {
                Some(as_drag_payload(ClipDragPayload {
                    clip_id,
                    original_start,
                    original_track,
                }))
            }
        })
        .on_drag_end(move |_| {}))
    .child((
        left_handle,
        Box(Modifier::new().flex_grow(1.0).padding(4.0))
            .child(Column(Modifier::new().fill_max_width()).child((label_view, waveform))),
        right_handle,
    ));

    let store_for_click = store.clone();
    let mut stack_children: Vec<View> = vec![
        Button(clip_content, {
            move || {
                store_for_click.state.selected_clip_id.set(Some(clip_id));
                store_for_click.state.selected_asset_id.set(None);
            }
        })
        .modifier(Modifier::new().on_action({
            let store = store.clone();
            move |action| {
                if let repose_core::shortcuts::Action::Custom(name) = action {
                    if name.as_ref() == "timeline:delete" {
                        store.dispatch_edit(EditCommand::RemoveClip { clip_id });
                        store.state.selected_clip_id.set(None);
                        return true;
                    }
                }
                false
            }
        })),
    ];

    if clip.transition_in.is_some() {
        stack_children.push(transition_indicator(render_w, clip_h, true));
    }
    if clip.transition_out.is_some() {
        stack_children.push(transition_indicator(render_w, clip_h, false));
    }

    Stack(Modifier::new().size(render_w, clip_h)).child(stack_children)
}

fn clip_thumbnails(store: Rc<Store>, clip: &Clip, width: f32) -> View {
    let Some(asset_id) = (|| {
        let source_path = match &clip.kind {
            ClipKind::Video(v) => v.source_path.as_str(),
            _ => return None,
        };
        let assets = store.state.assets.get();
        let asset = assets
            .iter()
            .find(|a| a.effective_path().to_string_lossy().as_ref() == source_path)?;
        Some(asset.id)
    })() else {
        return Box(Modifier::new().width(width).height(1.0));
    };

    let num_thumbnails = ((width / 100.0).ceil() as usize).max(2).min(20);
    let dur_us = clip.timeline_duration.as_micros().max(1);
    let thumb_height = 40.0;

    let mut handles: Vec<(f32, repose_core::ImageHandle)> = Vec::new();
    for i in 0..num_thumbnails {
        let t = (i as f64 + 0.5) / num_thumbnails as f64;
        let source_time = (t * dur_us as f64) as i64;
        let key = (asset_id, source_time);

        let cached = store
            .timeline_thumb_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&key).copied());

        if let Some(handle) = cached {
            handles.push((1.0 / num_thumbnails as f32, handle));
        } else {
            store.dispatch_preview(PreviewCommand::RequestTimelineThumbnail {
                asset_id,
                source_time,
            });
        }
    }

    if handles.is_empty() {
        return Box(Modifier::new().width(width).height(thumb_height));
    }

    let children: Vec<View> = handles
        .into_iter()
        .map(|(frac, handle)| {
            Image(
                Modifier::new()
                    .width(width * frac)
                    .height(thumb_height),
                handle,
            )
            .image_fit(repose_core::ImageFit::Cover)
        })
        .collect();

    Row(Modifier::new().width(width).height(thumb_height)).child(children)
}
