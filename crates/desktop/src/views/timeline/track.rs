//! Track headers, lanes, and the add-track row.

use crate::state::Store;
use crate::views::dnd::{AssetDragPayload, ClipDragPayload, TrimPayload};
use crate::views::timeline::clip::clip_view;
use crate::views::timeline::geometry::{
    snap_candidates, snap_edge, snap_moving_clip, window_to_us, TimelineScale,
    ADD_TRACK_ROW_HEIGHT, TRACK_HEADER_WIDTH, TRACK_HEIGHT,
};
use miniter_domain::{Clip, ClipId, ClipKind, MediaDuration, Timestamp, Track, TrackId, TrackKind};
use miniter_usecases::EditCommand;
use repose_core::{
    dnd::{DragOver, DropEvent},
    input::{PointerButton, PointerEventKind},
    CursorIcon, Modifier, Vec2, View,
};
use repose_material::Icon;
use repose_ui::{scroll::ScrollStateXY, Box, Column, Row, Text, TextStyle, ViewExt};
use snapshort_ui_core::{colors, Icons};
use snapshort_usecases::{AssetType, PlaybackCommand};
use std::rc::Rc;

fn kind_color(kind: TrackKind) -> repose_core::Color {
    match kind {
        TrackKind::Video => colors::VIDEO_TRACK,
        TrackKind::Audio => colors::AUDIO_TRACK,
        TrackKind::Text => colors::ACCENT,
        _ => colors::TEXT_MUTED,
    }
}

fn kind_icon(kind: TrackKind) -> repose_material::Symbol {
    match kind {
        TrackKind::Video => Icons::movie,
        TrackKind::Audio => Icons::music_note,
        TrackKind::Text => Icons::text_fields,
        TrackKind::Subtitle => Icons::subtitle,
        _ => Icons::movie,
    }
}

/// Miniter-style compact header: type icon (dimmed when muted) + small lock
/// badge. All actions live in the right-click menu.
pub fn track_header(store: Rc<Store>, track: &Track) -> View {
    let track_id = track.id;
    let muted = track.muted;
    let locked = track.locked;
    let store_for_menu = store.clone();

    let icon = kind_icon(track.kind);
    let icon_color = if muted {
        colors::TEXT_DISABLED
    } else {
        kind_color(track.kind)
    };

    Column(
        Modifier::new()
            .width(TRACK_HEADER_WIDTH)
            .height(TRACK_HEIGHT)
            .background(colors::BG_PANEL)
            .border(1.0, colors::BORDER, 0.0)
            .align_items(repose_core::AlignItems::CENTER)
            .justify_content(repose_core::AlignContent::CENTER)
            .cursor(CursorIcon::Pointer)
            .on_pointer_down(move |event| {
                if matches!(
                    &event.event,
                    PointerEventKind::Down(PointerButton::Secondary)
                ) {
                    let window_pos = event.position_in_window();
                    store_for_menu.open_track_menu(window_pos, track_id);
                }
            }),
    )
    .child(
        Column(
            Modifier::new()
                .align_items(repose_core::AlignItems::CENTER)
                .justify_content(repose_core::AlignContent::CENTER),
        )
        .child((
            Icon(icon).size(16.0).color(icon_color),
            if locked {
                Icon(Icons::lock).size(9.0).color(colors::WARNING)
            } else {
                Box(Modifier::new().width(9.0).height(2.0))
            },
        )),
    )
}

pub fn add_track_row(store: Rc<Store>) -> View {
    let store_for_menu = store.clone();

    Box(Modifier::new()
        .width(1.0)
        .height(ADD_TRACK_ROW_HEIGHT)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .align_items(repose_core::AlignItems::CENTER)
        .justify_content(repose_core::AlignContent::CENTER)
        .clickable()
        .on_pointer_down(move |event| {
            let window_pos = event.position_in_window();
            store_for_menu.open_add_track_menu(window_pos);
        })
        .cursor(CursorIcon::Pointer))
    .child(
        Box(Modifier::new()
            .height(20.0)
            .padding_values(repose_core::PaddingValues {
                left: 10.0,
                right: 10.0,
                top: 0.0,
                bottom: 0.0,
            })
            .background(colors::BG_HOVER)
            .clip_rounded(10.0))
        .child(
            Row(Modifier::new()
                .height(20.0)
                .align_items(repose_core::AlignItems::CENTER))
            .child((
                Icon(Icons::add).size(14.0).color(colors::TEXT_ACCENT),
                Box(Modifier::new().width(4.0)),
                Text("Add Track")
                    .size(10.0)
                    .color(colors::TEXT_ACCENT)
                    .single_line(),
            )),
        ),
    )
}

pub fn track_lane(
    store: Rc<Store>,
    track: &Track,
    scale: TimelineScale,
    scroll_state_xy: Rc<ScrollStateXY>,
    panel_origin: Vec2,
) -> View {
    let track_id = track.id;
    let kind = track.kind;
    let locked = track.locked;

    let mut clips: Vec<&Clip> = track.clips.iter().collect();
    clips.sort_by_key(|c| c.timeline_start.0);
    let selected_clip = store.state.selected_clip_id.get();

    let mut children: Vec<View> = Vec::new();
    for clip in clips {
        children.push(clip_view(
            store.clone(),
            clip,
            kind,
            scale,
            scroll_state_xy.clone(),
            panel_origin,
            selected_clip,
            track_id,
            locked,
        ));
    }

    let store_for_drop = store.clone();
    let bg = if store.state.drag_hover_track.get() == Some(track_id) {
        colors::BG_HOVER
    } else {
        colors::BG_TRACK
    };

    let lane = Column(
        Modifier::new()
            .width(1.0)
            .height(TRACK_HEIGHT)
            .background(bg)
            .border(
                1.0,
                if locked {
                    colors::WARNING
                } else {
                    colors::BORDER
                },
                0.0,
            )
            .on_pointer_down({
                let store = store.clone();
                let scroll_state_xy = scroll_state_xy.clone();
                move |event| {
                    // PointerEvent.position is local to the lane; convert to window
                    // space, then to timeline us (see geometry::window_to_us).
                    let (scroll_x, _) = scroll_state_xy.get();
                    let us =
                        window_to_us(event.position_in_window().x, panel_origin, scroll_x, scale);
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
                let scroll_state_xy = scroll_state_xy.clone();
                move |_| {
                    store.state.drag_hover_track.set(None);
                    if store.state.timeline_snap.get() {
                        store.state.timeline_snap_indicator.set(None);
                    }
                    let _ = scroll_state_xy;
                }
            })
            .on_drag_over({
                let store = store.clone();
                let scroll_state_xy = scroll_state_xy.clone();
                move |event: DragOver| {
                    if !store.state.timeline_snap.get() || locked {
                        store.state.timeline_snap_indicator.set(None);
                        return;
                    }
                    let (scroll_x, _) = scroll_state_xy.get();
                    let dropped_us = window_to_us(event.position.x, panel_origin, scroll_x, scale);

                    if let Some(payload) = event.payload.downcast_ref::<ClipDragPayload>() {
                        let Some(timeline) = store.state.timeline.get() else {
                            return;
                        };
                        let Some(clip) = timeline
                            .tracks
                            .iter()
                            .find_map(|t| t.clip_by_id(payload.clip_id))
                        else {
                            return;
                        };
                        let desired_start_us =
                            dropped_us.saturating_sub(payload.grab_offset_us).max(0);
                        let snap = snap_moving_clip(
                            &timeline,
                            clip.id,
                            desired_start_us,
                            clip.timeline_duration.as_micros(),
                            store.state.playhead.get().0,
                            store
                                .state
                                .timeline_markers
                                .get()
                                .iter()
                                .map(|m| m.timestamp_us),
                            scale,
                        );
                        store
                            .state
                            .timeline_snap_indicator
                            .set(snap.guide_us.map(Timestamp));
                    } else if let Some(payload) = event.payload.downcast_ref::<TrimPayload>() {
                        let Some(timeline) = store.state.timeline.get() else {
                            return;
                        };
                        let Some(clip) = timeline
                            .tracks
                            .iter()
                            .find_map(|t| t.clip_by_id(payload.clip_id))
                        else {
                            return;
                        };
                        let candidates = snap_candidates(
                            &timeline,
                            clip.id,
                            store.state.playhead.get().0,
                            store
                                .state
                                .timeline_markers
                                .get()
                                .iter()
                                .map(|m| m.timestamp_us),
                        );
                        let snap = snap_edge(candidates.into_iter(), dropped_us, scale);
                        store
                            .state
                            .timeline_snap_indicator
                            .set(snap.guide_us.map(Timestamp));
                    }
                }
            })
            .on_drop({
                let store = store.clone();
                let scroll_state_xy = scroll_state_xy.clone();
                move |event: DropEvent| {
                    store.state.drag_hover_track.set(None);
                    let (scroll_x, _scroll_y) = scroll_state_xy.get();

                    if store_for_drop.state.timeline_snap.get() {
                        store_for_drop.state.timeline_snap_indicator.set(None);
                    }

                    if locked {
                        return false;
                    }

                    if let Some(payload) = event.payload.downcast_ref::<TrimPayload>() {
                        let payload = payload.clone();
                        return handle_trim(
                            &event,
                            store_for_drop.clone(),
                            &payload,
                            scale,
                            panel_origin,
                            scroll_x,
                        );
                    }
                    if let Some(payload) = event.payload.downcast_ref::<ClipDragPayload>() {
                        let payload = payload.clone();
                        return handle_clip_move(
                            &event,
                            store_for_drop.clone(),
                            &payload,
                            track_id,
                            kind,
                            scale,
                            panel_origin,
                            scroll_x,
                        );
                    }
                    if let Some(payload) = event.payload.downcast_ref::<AssetDragPayload>() {
                        let payload = payload.clone();
                        return handle_asset_drop(
                            &event,
                            store_for_drop.clone(),
                            &payload,
                            track_id,
                            kind,
                            scale,
                            panel_origin,
                            scroll_x,
                        );
                    }
                    false
                }
            }),
    )
    .child(children);

    lane
}

fn handle_trim(
    event: &DropEvent,
    store: Rc<Store>,
    payload: &TrimPayload,
    scale: TimelineScale,
    panel_origin: Vec2,
    scroll_x: f32,
) -> bool {
    let timeline = store.state.timeline.get();
    let Some(timeline) = timeline else {
        return false;
    };
    let Some(clip) = timeline
        .tracks
        .iter()
        .find_map(|t| t.clip_by_id(payload.clip_id))
    else {
        return false;
    };

    // Trim edges snap against other clip edges / playhead / markers / zero.
    let mut trim_us = window_to_us(event.position.x, panel_origin, scroll_x, scale);
    if store.state.timeline_snap.get() {
        let candidates = snap_candidates(
            &timeline,
            payload.clip_id,
            store.state.playhead.get().0,
            store
                .state
                .timeline_markers
                .get()
                .iter()
                .map(|m| m.timestamp_us),
        );
        trim_us = snap_edge(candidates.into_iter(), trim_us, scale).value_us;
    }
    store.state.timeline_snap_indicator.set(None);

    if payload.is_start {
        if trim_us <= clip.timeline_start.0 || trim_us >= clip.timeline_end().as_micros() {
            return true;
        }
        let new_start = Timestamp(trim_us.max(0));
        let delta = clip.timeline_start - new_start;
        let new_source_start = clip.source_start + delta;
        store.dispatch_edit(EditCommand::TrimClipStart {
            clip_id: payload.clip_id,
            new_start,
            new_source_start,
        });
    } else {
        if trim_us <= clip.timeline_start.0 {
            return true;
        }
        let new_duration = MediaDuration::from_micros((trim_us - clip.timeline_start.0).max(1));
        store.dispatch_edit(EditCommand::TrimClipEnd {
            clip_id: payload.clip_id,
            new_duration,
        });
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn handle_clip_move(
    event: &DropEvent,
    store: Rc<Store>,
    payload: &ClipDragPayload,
    target_track_id: TrackId,
    target_kind: TrackKind,
    scale: TimelineScale,
    panel_origin: Vec2,
    scroll_x: f32,
) -> bool {
    let timeline = store.state.timeline.get();
    let Some(timeline) = timeline else {
        return false;
    };

    let Some(clip) = timeline
        .tracks
        .iter()
        .find_map(|t| t.clip_by_id(payload.clip_id))
    else {
        return false;
    };

    if !clip_kind_matches(clip, target_kind) {
        return false;
    }

    let dropped_us = window_to_us(event.position.x, panel_origin, scroll_x, scale);
    let desired_start_us = dropped_us.saturating_sub(payload.grab_offset_us).max(0);
    let duration_us = clip.timeline_duration.as_micros();

    let snap_result = if store.state.timeline_snap.get() {
        let playhead_us = store.state.playhead.get().0;
        let markers = store.state.timeline_markers.get();
        snap_moving_clip(
            &timeline,
            clip.id,
            desired_start_us,
            duration_us,
            playhead_us,
            markers.iter().map(|m| m.timestamp_us),
            scale,
        )
    } else {
        super::geometry::SnapResult {
            value_us: desired_start_us,
            guide_us: None,
        }
    };

    if payload.original_start.0 == snap_result.value_us && payload.original_track == target_track_id
    {
        return true;
    }

    store.dispatch_edit(EditCommand::MoveClip {
        clip_id: payload.clip_id,
        new_track_id: target_track_id,
        new_start: Timestamp(snap_result.value_us),
    });

    // Guide is painted live during on_drag_over; clear it once the clip lands.
    store.state.timeline_snap_indicator.set(None);
    true
}

fn clip_kind_matches(clip: &Clip, target_kind: TrackKind) -> bool {
    let clip_kind = match &clip.kind {
        ClipKind::Video(_) => TrackKind::Video,
        ClipKind::Audio(_) => TrackKind::Audio,
        ClipKind::Text(_) => TrackKind::Text,
        ClipKind::Subtitle(_) => TrackKind::Subtitle,
        _ => return false,
    };
    clip_kind == target_kind
}

#[allow(clippy::too_many_arguments)]
fn handle_asset_drop(
    event: &DropEvent,
    store: Rc<Store>,
    payload: &AssetDragPayload,
    track_id: TrackId,
    kind: TrackKind,
    scale: TimelineScale,
    panel_origin: Vec2,
    scroll_x: f32,
) -> bool {
    let drop_us = window_to_us(event.position.x, panel_origin, scroll_x, scale);

    let assets = store.state.assets.get();
    let Some(asset) = assets.iter().find(|a| a.id == payload.asset_id) else {
        return false;
    };

    if !asset.status.is_usable() {
        store
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
        .and_then(|m| m.primary_video().map(|v| (v.width, v.height, v.fps)))
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
            masks: vec![],
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
        blend_mode: Default::default(),
    };

    store.dispatch_edit(EditCommand::AddClip { track_id, clip });
    true
}
