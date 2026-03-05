use super::dnd::{as_drag_payload, AssetDragPayload, ClipDragPayload, TrimPayload};
use crate::state::Store;
use repose_core::{
    dnd::{DragOver, DragPayload, DragStart, DropEvent},
    view::View,
    Color, CursorIcon, Modifier,
};
use repose_ui::{
    scroll::{remember_scroll_state_xy, ScrollAreaXY},
    Box, Button, Column, ImageExt, Row, Slider, Stack, Text, TextStyle, ViewExt,
};
use snapshort_domain::{AssetType, Clip, ClipId, ClipType, Frame, Timeline, TrackRef, TrackType};
use snapshort_ui_core::{audio_waveform, colors};
use snapshort_usecases::{PlaybackCommand, TimelineCommand};
use std::rc::Rc;

fn h_spacer(w: f32) -> View {
    Box(Modifier::new().width(w))
}

trait TrackTypeUi {
    fn color(&self) -> Color;
    fn bg_color(&self) -> Color;
    fn label(&self, index: usize) -> String;
}

impl TrackTypeUi for TrackType {
    fn color(&self) -> Color {
        match self {
            TrackType::Video => colors::VIDEO_TRACK,
            TrackType::Audio => colors::AUDIO_TRACK,
        }
    }

    fn bg_color(&self) -> Color {
        match self {
            TrackType::Video => Color::from_rgb(0x1E, 0x3A, 0x5F),
            TrackType::Audio => Color::from_rgb(0x2D, 0x5A, 0x27),
        }
    }

    fn label(&self, index: usize) -> String {
        match self {
            TrackType::Video => format!("V{}", index + 1),
            TrackType::Audio => format!("A{}", index + 1),
        }
    }
}

pub fn timeline_panel(store: Rc<Store>) -> View {
    let timeline = store.state.timeline.get();

    let name = timeline
        .as_ref()
        .map(|t| t.name.clone())
        .unwrap_or_else(|| "No Timeline".into());

    let total_frames = timeline.as_ref().map(|t| t.duration().0).unwrap_or(0);
    let fps = timeline
        .as_ref()
        .map(|t| t.settings.fps)
        .unwrap_or(snapshort_domain::Fps::default());
    let timecode = frames_to_timecode(total_frames, fps);

    let playhead_frame = timeline.as_ref().map(|t| t.playhead.0).unwrap_or(0);
    let playhead_tc = frames_to_timecode(playhead_frame, fps);

    let px_per_frame = store.state.timeline_zoom.get();
    let track_scroll_xy_state = remember_scroll_state_xy("timeline_tracks_xy");

    let store_for_playhead = store.clone();
    let store_for_zoom = store.clone();
    let store_for_snap = store.clone();
    let store_for_split = store.clone();

    // Track header views
    let mut track_header_views: Vec<View> = Vec::new();
    track_header_views.push(Box(Modifier::new()
        .fill_max_width()
        .height(24.0)
        .background(colors::BG_PANEL)));

    if let Some(tl) = &timeline {
        for track in tl.video_tracks.iter() {
            let key = track.index as u64;
            track_header_views.push(track_header(&track.name, TrackType::Video, key));
        }
        for track in tl.audio_tracks.iter() {
            let key = 1000 + track.index as u64;
            track_header_views.push(track_header(&track.name, TrackType::Audio, key));
        }
    } else {
        track_header_views.push(track_header("V1", TrackType::Video, 0));
        track_header_views.push(track_header("A1", TrackType::Audio, 1000));
    }

    track_header_views.push(track_add_buttons(store.clone()));

    // Track content views
    let mut track_content_views: Vec<View> = Vec::new();
    track_content_views.push(time_ruler(
        store.clone(),
        track_scroll_xy_state.clone(),
        px_per_frame,
        total_frames,
    ));

    if let Some(tl) = &timeline {
        for track in tl.video_tracks.iter() {
            track_content_views.push(track_lane(
                store.clone(),
                tl,
                TrackType::Video,
                track.index,
                px_per_frame,
                track_scroll_xy_state.clone(),
            ));
        }
        for track in tl.audio_tracks.iter() {
            track_content_views.push(track_lane(
                store.clone(),
                tl,
                TrackType::Audio,
                track.index,
                px_per_frame,
                track_scroll_xy_state.clone(),
            ));
        }
    } else {
        track_content_views.push(empty_lane(TrackType::Video));
        track_content_views.push(empty_lane(TrackType::Audio));
    }

    // Left side: timeline name + settings
    let info = timeline
        .as_ref()
        .map(|t| {
            let fps_value = t.settings.fps.as_f64();
            let fps_label = if (fps_value - fps_value.round()).abs() < 0.01 {
                format!("{:.0}", fps_value)
            } else {
                format!("{:.2}", fps_value)
            };
            format!(
                "{}x{} | {}fps",
                t.settings.resolution.width, t.settings.resolution.height, fps_label
            )
        })
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

    // Center: tools (split + snap + zoom)
    let header_tools = Row(Modifier::new().align_items(repose_core::AlignItems::Center)).child((
        tool_group(vec![
            tool_icon_button("✂", {
                let store = store_for_split.clone();
                move || {
                    if let (Some(clip_id), Some(tl)) = (
                        store.state.selected_clip_id.get(),
                        store.state.timeline.get(),
                    ) {
                        store.dispatch_timeline(TimelineCommand::SplitAt {
                            clip_id,
                            frame: tl.playhead,
                        });
                    }
                }
            }),
            snap_toggle(store_for_snap),
        ]),
        h_spacer(10.0),
        tool_group(vec![
            Text("Zoom").size(10.0).color(colors::TEXT_MUTED),
            h_spacer(6.0),
            Slider(px_per_frame, (0.5, 12.0), None, {
                let store = store_for_zoom.clone();
                move |value| store.state.timeline_zoom.set(value)
            })
            .modifier(Modifier::new().width(90.0).height(18.0)),
        ]),
    ));

    // Right side: timecode display
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

    let track_header_scroll = track_scroll_xy_state.clone();
    let content = Row(Modifier::new().fill_max_size().flex_grow(1.0)).child((
        ScrollAreaXY(
            Modifier::new().width(200.0).fill_max_height(),
            track_header_scroll.clone(),
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
            playhead_at_scroll(playhead_frame, px_per_frame, track_scroll_xy_state, {
                let store = store_for_playhead.clone();
                move |frame| {
                    let snapped = if store.state.timeline_snap.get() {
                        let tl = store.state.timeline.get();
                        if let Some(tl) = tl {
                            let fps = tl.settings.fps.as_f64().round() as i64;
                            if fps > 0 {
                                let sec = ((frame as f64) / (fps as f64)).round() as i64;
                                sec * fps
                            } else {
                                frame
                            }
                        } else {
                            frame
                        }
                    } else {
                        frame
                    };
                    store.dispatch_playback(PlaybackCommand::Seek {
                        frame: Frame(snapped.max(0)),
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

fn track_header(name: &str, track_type: TrackType, key: u64) -> View {
    Row(Modifier::new()
        .key(key)
        .fill_max_width()
        .height(track_row_height(track_type))
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(6.0)
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        Box(Modifier::new()
            .width(12.0)
            .height(12.0)
            .border(2.0, track_type.color(), 0.0)),
        h_spacer(6.0),
        Text(name)
            .size(11.0)
            .color(colors::TEXT_PRIMARY)
            .single_line(),
    ])
}

fn track_row_height(track_type: TrackType) -> f32 {
    match track_type {
        TrackType::Video => 64.0,
        TrackType::Audio => 52.0,
    }
}

fn clip_row_height(track_type: TrackType) -> f32 {
    match track_type {
        TrackType::Video => 52.0,
        TrackType::Audio => 40.0,
    }
}

fn track_add_buttons(store: Rc<Store>) -> View {
    Row(Modifier::new()
        .fill_max_width()
        .height(44.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(6.0)
        .align_items(repose_core::AlignItems::Center))
    .child((
        tool_icon_button("+V", {
            let store = store.clone();
            move || store.dispatch_timeline(TimelineCommand::AddVideoTrack)
        }),
        h_spacer(8.0),
        tool_icon_button("+A", {
            let store = store.clone();
            move || store.dispatch_timeline(TimelineCommand::AddAudioTrack)
        }),
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

fn tool_icon_button(icon: &str, on_click: impl Fn() + 'static) -> View {
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
    .child(Text(icon).color(th.primary).size(12.0).single_line())
}

fn time_ruler(
    store: Rc<Store>,
    scroll_state_xy: std::rc::Rc<repose_ui::scroll::ScrollStateXY>,
    px_per_frame: f32,
    total_frames: i64,
) -> View {
    // Calculate marker spacing based on zoom
    let fps = store
        .state
        .timeline
        .get()
        .map(|t| t.settings.fps)
        .unwrap_or(snapshort_domain::Fps::default());
    let frames_per_second: i64 = fps.as_f64().round() as i64;
    let seconds_per_marker: i64 = if px_per_frame > 5.0 {
        1
    } else if px_per_frame > 2.0 {
        2
    } else if px_per_frame > 1.0 {
        5
    } else {
        10
    };

    let marker_width = (frames_per_second * seconds_per_marker) as f32 * px_per_frame;

    let total_seconds = ((total_frames.max(0) as f32) / frames_per_second as f32).ceil() as i64;
    let marker_count = ((total_seconds / seconds_per_marker) + 2).clamp(2, 240);

    let mut markers: Vec<View> = Vec::new();
    for i in 0..marker_count {
        let seconds = i * seconds_per_marker;
        let tc = format!(
            "{:02}:{:02}:{:02}:00",
            seconds / 3600,
            (seconds % 3600) / 60,
            seconds % 60
        );
        markers.push(time_marker(&tc, marker_width));
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
            let scroll_state_xy = scroll_state_xy.clone();
            move |event| {
                let (scroll_x, _scroll_y) = scroll_state_xy.get();
                seek_at_x(&store, px_per_frame, event.position.x + scroll_x);
            }
        }))
    .child(markers)
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

fn empty_lane(track_type: TrackType) -> View {
    Row(Modifier::new()
        .fill_max_width()
        .height(track_row_height(track_type))
        .background(colors::BG_TRACK)
        .border(1.0, colors::BORDER, 0.0)
        .padding(4.0))
    .child((
        Box(Modifier::new().flex_grow(1.0)),
        Text(match track_type {
            TrackType::Video => "No clips (V)",
            TrackType::Audio => "No clips (A)",
        })
        .size(10.0)
        .color(colors::TEXT_DISABLED),
        Box(Modifier::new().flex_grow(1.0)),
    ))
}

fn track_lane(
    store: Rc<Store>,
    timeline: &Timeline,
    track_type: TrackType,
    track_index: usize,
    px_per_frame: f32,
    scroll_state_xy: std::rc::Rc<repose_ui::scroll::ScrollStateXY>,
) -> View {
    let mut clips: Vec<Clip> = timeline
        .clips_on_track(TrackRef {
            track_type,
            index: track_index,
        })
        .cloned()
        .collect();
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
        let gap_frames = (start - cursor).max(0);
        if gap_frames > 0 {
            children.push(Box(Modifier::new().width(gap_frames as f32 * px_per_frame)));
        }
        children.push(clip_view(
            store.clone(),
            clip,
            track_type,
            px_per_frame,
            selected_clip,
        ));
        cursor = clip.timeline_end().0;
    }

    children.push(Box(Modifier::new().flex_grow(1.0)));

    // Make the track a drop target
    let store_for_drop = store.clone();
    let store_for_drag_over = store.clone();

    Row(Modifier::new()
        .fill_max_width()
        .height(track_row_height(track_type))
        .background(colors::BG_TRACK)
        .border(1.0, colors::BORDER, 0.0)
        .padding(4.0)
        .align_items(repose_core::AlignItems::Center)
        .on_pointer_down({
            let store = store.clone();
            let scroll_state_xy = scroll_state_xy.clone();
            move |event| {
                let (scroll_x, _scroll_y) = scroll_state_xy.get();
                seek_at_x(&store, px_per_frame, event.position.x + scroll_x);
                store.state.selected_clip_id.set(None);
                store.state.selected_asset_id.set(None);
            }
        })
        .on_drag_over({
            let scroll_state_xy = scroll_state_xy.clone();
            move |event: DragOver| {
                let (scroll_x, _scroll_y) = scroll_state_xy.get();
                let drag_frame = frame_from_x(
                    &store_for_drag_over,
                    px_per_frame,
                    event.position.x + scroll_x,
                );
                if let Some(payload) = event.payload.downcast_ref::<TrimPayload>() {
                    if payload.is_start {
                        let min_frame = payload.original_frame.0 + 1;
                        if drag_frame >= min_frame {
                            store_for_drag_over.dispatch_timeline(TimelineCommand::TrimStart {
                                clip_id: payload.clip_id,
                                new_start: Frame(drag_frame.max(0)),
                            });
                        }
                    } else {
                        let max_frame = payload.original_frame.0;
                        if drag_frame <= max_frame {
                            store_for_drag_over.dispatch_timeline(TimelineCommand::TrimEnd {
                                clip_id: payload.clip_id,
                                new_end: Frame(drag_frame.max(0)),
                            });
                        }
                    }
                }
            }
        })
        .on_drop({
            let track_type = track_type;
            let track_index = track_index;
            let scroll_state_xy = scroll_state_xy.clone();
            move |event: DropEvent| {
                let (scroll_x, _scroll_y) = scroll_state_xy.get();
                let drop_frame =
                    frame_from_x(&store_for_drop, px_per_frame, event.position.x + scroll_x);
                if let Some(payload) = event.payload.downcast_ref::<TrimPayload>() {
                    if payload.is_start {
                        let min_frame = payload.original_frame.0 + 1;
                        if drop_frame < min_frame {
                            return true;
                        }
                        store_for_drop.dispatch_timeline(TimelineCommand::TrimStart {
                            clip_id: payload.clip_id,
                            new_start: Frame(drop_frame.max(0)),
                        });
                    } else {
                        let max_frame = payload.original_frame.0;
                        if drop_frame > max_frame {
                            return true;
                        }
                        store_for_drop.dispatch_timeline(TimelineCommand::TrimEnd {
                            clip_id: payload.clip_id,
                            new_end: Frame(drop_frame.max(0)),
                        });
                    }
                    return true;
                }
                if let Some(payload) = event.payload.downcast_ref::<ClipDragPayload>() {
                    store_for_drop.dispatch_timeline(TimelineCommand::MoveClip {
                        clip_id: payload.clip_id,
                        new_start: Frame(drop_frame.max(0)),
                        new_track: TrackRef {
                            track_type,
                            index: track_index,
                        },
                    });
                    return true;
                }
                if let Some(payload) = event.payload.downcast_ref::<AssetDragPayload>() {
                    let assets = store_for_drop.state.assets.get();
                    let Some(asset) = assets.iter().find(|a| a.id == payload.asset_id) else {
                        return false;
                    };

                    let allowed = match track_type {
                        TrackType::Video => matches!(
                            asset.asset_type,
                            AssetType::Video | AssetType::Image | AssetType::Sequence
                        ),
                        TrackType::Audio => matches!(asset.asset_type, AssetType::Audio),
                    };

                    if !allowed {
                        return false;
                    }

                    store_for_drop.dispatch_timeline(TimelineCommand::InsertClip {
                        asset_id: payload.asset_id,
                        timeline_start: Frame(drop_frame.max(0)),
                        track: TrackRef {
                            track_type,
                            index: track_index,
                        },
                        source_range: None,
                    });
                    return true;
                }
                false
            }
        }))
    .child(children)
}

fn seek_at_x(store: &Store, px_per_frame: f32, x: f32) {
    if px_per_frame <= 0.0 {
        return;
    }
    let frame = frame_from_x(store, px_per_frame, x);
    store.dispatch_playback(PlaybackCommand::Seek {
        frame: Frame(frame.max(0)),
    });
}

fn frame_from_x(store: &Store, px_per_frame: f32, x: f32) -> i64 {
    if px_per_frame <= 0.0 {
        return 0;
    }
    let raw = (x / px_per_frame).round() as i64;
    if !store.state.timeline_snap.get() {
        return raw;
    }

    let timeline = store.state.timeline.get();
    let Some(tl) = timeline else {
        return raw;
    };

    let fps = tl.settings.fps.as_f64().round() as i64;
    let mut candidates: Vec<i64> = Vec::new();
    candidates.push(0);
    candidates.push(tl.playhead.0);
    candidates.push(tl.duration().0);

    if fps > 0 {
        let sec = ((raw as f64) / (fps as f64)).round() as i64;
        candidates.push((sec * fps).max(0));
    }

    for clip in tl.clips.iter() {
        candidates.push(clip.timeline_start.0);
        candidates.push(clip.timeline_end().0);
    }

    let snap_threshold = 8.0_f32; // px
    let mut best = raw;
    let mut best_dist = snap_threshold + 1.0;
    for c in candidates {
        let dist = ((c - raw).abs() as f32) * px_per_frame;
        if dist <= snap_threshold && dist < best_dist {
            best = c;
            best_dist = dist;
        }
    }

    best
}

fn playhead_at_scroll(
    playhead_frame: i64,
    px_per_frame: f32,
    scroll_state_xy: std::rc::Rc<repose_ui::scroll::ScrollStateXY>,
    on_seek: impl Fn(i64) + 'static,
) -> View {
    let (scroll_x, _scroll_y) = scroll_state_xy.get();
    let x = playhead_frame as f32 * px_per_frame - scroll_x;
    let line_color = colors::ACCENT;
    let seek_px = px_per_frame;
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
                let frame = ((event.position.x + scroll_x) / seek_px).round() as i64;
                on_seek(frame.max(0));
            }),
    )
}

fn clip_view(
    store: Rc<Store>,
    clip: &Clip,
    track_type: TrackType,
    px_per_frame: f32,
    selected_clip: Option<ClipId>,
) -> View {
    let dur = clip.effective_duration().max(1);
    let render_w = (dur as f32 * px_per_frame).max(1.0);

    let (bg, border, label) = match clip.clip_type {
        ClipType::Gap => (colors::BG_LIGHT, colors::BORDER, "Gap".to_string()),
        _ => (
            track_type.bg_color(),
            track_type.color(),
            clip.name.clone().unwrap_or_else(|| "Clip".to_string()),
        ),
    };

    let is_selected = selected_clip == Some(clip.id);
    let border_color = if is_selected { colors::ACCENT } else { border };
    let bg_color = if is_selected { colors::BG_SELECTED } else { bg };

    let clip_id = clip.id;
    let original_start = clip.timeline_start;
    let original_track = clip.track;

    let clip_h = clip_row_height(track_type);
    let show_details = render_w >= 64.0;
    let waveform = if track_type == TrackType::Audio && show_details {
        let waveform_width = (render_w - 24.0).max(10.0);
        let waveform_height = (clip_h - 18.0).clamp(8.0, 18.0);
        audio_waveform(waveform_width, waveform_height, None, colors::AUDIO_TRACK)
    } else if track_type != TrackType::Audio && show_details {
        clip_thumbnails(store.clone(), clip, render_w)
    } else {
        Box(Modifier::new().width(1.0).height(1.0))
    };
    let label_view: View = if show_details {
        Text(label).size(10.0).color(colors::TEXT_PRIMARY)
    } else {
        Box(Modifier::new().width(1.0).height(1.0))
    };

    let store_for_select = store.clone();

    // Trim handles (left and right edges)
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
            let original_start = original_start;
            move |_: DragStart| -> Option<DragPayload> {
                Some(as_drag_payload(TrimPayload {
                    clip_id,
                    is_start: true,
                    original_frame: original_start,
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
            let end_frame = clip.timeline_end();
            move |_: DragStart| -> Option<DragPayload> {
                Some(as_drag_payload(TrimPayload {
                    clip_id,
                    is_start: false,
                    original_frame: end_frame,
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

    Button(clip_content, {
        move || {
            store_for_select.state.selected_clip_id.set(Some(clip_id));
            store_for_select.state.selected_asset_id.set(None);
        }
    })
    .modifier(Modifier::new().on_action({
        let store = store.clone();
        move |action| {
            if let repose_core::shortcuts::Action::Custom(name) = action {
                if name.as_ref() == "timeline:delete" {
                    store.dispatch_timeline(TimelineCommand::RippleDelete { clip_id });
                    store.state.selected_clip_id.set(None);
                    return true;
                }
            }
            false
        }
    }))
}

fn clip_thumbnails(store: Rc<Store>, clip: &Clip, width: f32) -> View {
    let Some(asset_id) = clip.asset_id else {
        return Box(Modifier::new().width(1.0).height(1.0));
    };

    let fps = store
        .state
        .timeline
        .get()
        .map(|t| t.settings.fps)
        .unwrap_or(snapshort_domain::Fps::default());

    let start_frame = clip.source_range.start.0;
    let end_frame = clip.source_range.end.0.saturating_sub(1);

    let start_handle = ensure_timeline_thumbnail(store.clone(), asset_id, start_frame, fps);
    let end_handle = ensure_timeline_thumbnail(store.clone(), asset_id, end_frame, fps);

    let thumb_w = 32.0_f32.min(width.max(16.0));
    let thumb_h = (thumb_w * 0.56).max(14.0).min(20.0);

    Row(Modifier::new().fill_max_width().height(thumb_h)).child((
        thumbnail_box(start_handle, thumb_w, thumb_h),
        Box(Modifier::new().flex_grow(1.0)),
        thumbnail_box(end_handle, thumb_w, thumb_h),
    ))
}

fn thumbnail_box(handle: Option<repose_core::ImageHandle>, width: f32, height: f32) -> View {
    let Some(handle) = handle else {
        return Box(Modifier::new()
            .width(width)
            .height(height)
            .background(colors::BG_LIGHT)
            .border(1.0, colors::BORDER, 2.0));
    };
    repose_ui::Image(Modifier::new().width(width).height(height), handle)
        .image_fit(repose_core::ImageFit::Contain)
}

fn ensure_timeline_thumbnail(
    store: Rc<Store>,
    asset_id: snapshort_domain::AssetId,
    source_frame: i64,
    fps: snapshort_domain::Fps,
) -> Option<repose_core::ImageHandle> {
    let key = (asset_id, source_frame);
    if let Ok(cache) = store.timeline_thumb_cache.lock() {
        if let Some(handle) = cache.get(&key) {
            return Some(*handle);
        }
    }

    if let Ok(mut in_flight) = store.timeline_thumb_in_flight.lock() {
        if in_flight.contains(&key) {
            return None;
        }
        in_flight.insert(key);
    }

    let assets = store.state.assets.get();
    let asset = assets.iter().find(|a| a.id == asset_id).cloned()?;
    let asset_path = asset.effective_path().clone();

    let render_ctx = store.render_ctx.borrow().clone()?;
    let handle = render_ctx.alloc_image_handle();

    let cache = store.timeline_thumb_cache.clone();
    let in_flight = store.timeline_thumb_in_flight.clone();
    let fps_value = fps.as_f64();

    std::thread::spawn(move || {
        let output = std::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-ss")
            .arg(format!("{:.3}", source_frame as f64 / fps_value))
            .arg("-i")
            .arg(asset_path)
            .arg("-vframes")
            .arg("1")
            .arg("-vf")
            .arg("scale=160:90:flags=lanczos")
            .arg("-f")
            .arg("image2")
            .arg("-")
            .output();

        let Ok(output) = output else {
            if let Ok(mut s) = in_flight.lock() {
                s.remove(&key);
            }
            return;
        };
        if !output.status.success() {
            if let Ok(mut s) = in_flight.lock() {
                s.remove(&key);
            }
            return;
        }

        let bytes = output.stdout;
        let rgba = match image::load_from_memory(&bytes) {
            Ok(img) => img.to_rgba8(),
            Err(_) => {
                if let Ok(mut s) = in_flight.lock() {
                    s.remove(&key);
                }
                return;
            }
        };

        let (w, h) = rgba.dimensions();
        render_ctx.set_image_rgba8(handle, w, h, rgba.into_raw(), true);
        if let Ok(mut cache) = cache.lock() {
            cache.insert(key, handle);
        }
        if let Ok(mut s) = in_flight.lock() {
            s.remove(&key);
        }
        repose_core::request_frame();
    });

    Some(handle)
}

fn frames_to_timecode(frames: i64, fps: snapshort_domain::Fps) -> String {
    let fps_int = fps.as_f64().round() as i64;
    if fps_int <= 0 {
        return "00:00:00:00".to_string();
    }
    let frames_per_hour = fps_int * 60 * 60;
    let frames_per_min = fps_int * 60;

    let hours = frames / frames_per_hour;
    let minutes = (frames % frames_per_hour) / frames_per_min;
    let seconds = (frames % frames_per_min) / fps_int;
    let frame_num = frames % fps_int;

    format!(
        "{:02}:{:02}:{:02}:{:02}",
        hours, minutes, seconds, frame_num
    )
}
