use super::dnd::{as_drag_payload, AssetDragPayload, ClipDragPayload, TrimPayload};
use crate::state::Store;
use repose_core::{
    dnd::{DragOver, DragPayload, DragStart, DropEvent},
    view::View,
    Color, CursorIcon, Modifier,
};
use repose_ui::{
    scroll::{remember_scroll_state, ScrollArea},
    Box, Button, Column, Row, Slider, Stack, Text, TextStyle, ViewExt,
};
use snapshort_domain::{AssetType, Clip, ClipId, ClipType, Frame, Timeline, TrackRef, TrackType};
use snapshort_ui_core::{colors, playhead};
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

    let video_track_count = timeline.as_ref().map(|t| t.video_tracks.len()).unwrap_or(1);
    let audio_track_count = timeline.as_ref().map(|t| t.audio_tracks.len()).unwrap_or(1);

    let total_frames = timeline.as_ref().map(|t| t.duration().0).unwrap_or(0);
    let timecode = frames_to_timecode(total_frames, 24);

    let playhead_frame = timeline.as_ref().map(|t| t.playhead.0).unwrap_or(0);
    let playhead_tc = frames_to_timecode(playhead_frame, 24);

    let px_per_frame = store.state.timeline_zoom.get();

    let store_for_playhead = store.clone();
    let store_for_zoom = store.clone();
    let store_for_split = store.clone();

    // Track header views
    let mut track_header_views: Vec<View> = Vec::new();
    track_header_views.push(Box(Modifier::new()
        .fill_max_width()
        .height(24.0)
        .background(colors::BG_PANEL)));

    for i in 0..video_track_count {
        track_header_views.push(track_header(
            &(TrackType::Video).label(i),
            TrackType::Video,
            i as u64,
        ));
    }
    for i in 0..audio_track_count {
        track_header_views.push(track_header(
            &(TrackType::Audio).label(i),
            TrackType::Audio,
            (video_track_count + i) as u64,
        ));
    }

    track_header_views.push(track_add_buttons(store.clone()));

    // Track content views
    let mut track_content_views: Vec<View> = Vec::new();
    track_content_views.push(time_ruler(px_per_frame, total_frames));

    if let Some(tl) = &timeline {
        for i in 0..tl.video_tracks.len() {
            track_content_views.push(track_lane(
                store.clone(),
                tl,
                TrackType::Video,
                i,
                px_per_frame,
            ));
        }
        for i in 0..tl.audio_tracks.len() {
            track_content_views.push(track_lane(
                store.clone(),
                tl,
                TrackType::Audio,
                i,
                px_per_frame,
            ));
        }
    } else {
        track_content_views.push(empty_lane(TrackType::Video));
        track_content_views.push(empty_lane(TrackType::Audio));
    }

    // Left side: timeline name
    let header_left = Row(Modifier::new().align_items(repose_core::AlignItems::Center))
        .child((Text(name).size(12.0).color(colors::TEXT_PRIMARY),));

    // Center: tools (split button)
    let header_tools = Row(Modifier::new().align_items(repose_core::AlignItems::Center)).child((
        snapshort_ui_core::icon_button("✂", {
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
        h_spacer(8.0),
        Text("Zoom:").size(11.0).color(colors::TEXT_MUTED),
        Slider(px_per_frame, (0.5, 10.0), None, {
            let store = store_for_zoom.clone();
            move |value| store.state.timeline_zoom.set(value)
        })
        .modifier(Modifier::new().width(100.0).height(20.0)),
    ));

    // Right side: timecode display
    let header_timecode =
        Row(Modifier::new().align_items(repose_core::AlignItems::Center)).child((
            Text(playhead_tc).size(11.0).color(colors::TEXT_ACCENT),
            h_spacer(4.0),
            Text("/").size(11.0).color(colors::TEXT_MUTED),
            h_spacer(4.0),
            Text(timecode).size(11.0).color(colors::TEXT_PRIMARY),
        ));

    let header = Row(Modifier::new()
        .fill_max_width()
        .height(28.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(8.0)
        .align_items(repose_core::AlignItems::Center))
    .child((
        header_left,
        Box(Modifier::new().flex_grow(1.0)),
        header_tools,
        h_spacer(8.0),
        header_timecode,
    ));

    let content = Row(Modifier::new().fill_max_size().flex_grow(1.0)).child((
        Column(
            Modifier::new()
                .width(180.0)
                .fill_max_height()
                .border(1.0, colors::BORDER, 0.0),
        )
        .child(track_header_views),
        Column(Modifier::new().fill_max_width().flex_grow(1.0)).child((Stack(
            Modifier::new().fill_max_size(),
        )
        .child((
            ScrollArea(
                Modifier::new().fill_max_size(),
                remember_scroll_state("timeline_tracks"),
                Column(Modifier::new().fill_max_width()).child(track_content_views),
            ),
            playhead(playhead_frame, px_per_frame, {
                let store = store_for_playhead.clone();
                move |frame| {
                    store.dispatch_playback(PlaybackCommand::Seek {
                        frame: Frame(frame),
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
        .height(40.0)
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
        Text(name).size(11.0).color(colors::TEXT_PRIMARY),
        Box(Modifier::new().flex_grow(1.0)),
        track_header_icon("👁"),
        h_spacer(4.0),
        track_header_icon("🔒"),
        h_spacer(4.0),
        track_header_icon("M"),
    ])
}

fn track_header_icon(icon: &str) -> View {
    Box(Modifier::new().width(16.0).height(16.0))
        .child(Text(icon).size(10.0).color(colors::TEXT_MUTED))
}

fn track_add_buttons(store: Rc<Store>) -> View {
    Row(Modifier::new()
        .fill_max_width()
        .height(40.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(6.0)
        .align_items(repose_core::AlignItems::Center))
    .child((
        snapshort_ui_core::icon_button("+V", {
            let store = store.clone();
            move || store.dispatch_timeline(TimelineCommand::AddVideoTrack)
        }),
        h_spacer(8.0),
        snapshort_ui_core::icon_button("+A", {
            let store = store.clone();
            move || store.dispatch_timeline(TimelineCommand::AddAudioTrack)
        }),
        Box(Modifier::new().flex_grow(1.0)),
        Text("Add Track").size(10.0).color(colors::TEXT_MUTED),
    ))
}

fn time_ruler(px_per_frame: f32, total_frames: i64) -> View {
    // Calculate marker spacing based on zoom
    let frames_per_second: i64 = 24;
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
        .align_items(repose_core::AlignItems::End))
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

fn audio_waveform_placeholder() -> View {
    let heights = [
        4.0, 10.0, 6.0, 12.0, 8.0, 14.0, 7.0, 11.0, 5.0, 9.0, 6.0, 10.0,
    ];
    let mut bars: Vec<View> = Vec::new();
    for (i, h) in heights.iter().enumerate() {
        bars.push(Box(Modifier::new()
            .width(3.0)
            .height(*h)
            .background(colors::TEXT_MUTED)));
        if i + 1 < heights.len() {
            bars.push(Box(Modifier::new().width(2.0).height(1.0)));
        }
    }

    Row(Modifier::new().align_items(repose_core::AlignItems::End)).child(bars)
}

fn empty_lane(track_type: TrackType) -> View {
    Row(Modifier::new()
        .fill_max_width()
        .height(40.0)
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
) -> View {
    let mut clips: Vec<Clip> = timeline
        .clips_on_track(TrackRef {
            track_type,
            index: track_index,
        })
        .cloned()
        .collect();
    clips.sort_by_key(|c| c.timeline_start.0);

    let selected_clip = store.state.selected_clip_id.get();

    let mut children: Vec<View> = Vec::new();
    let mut cursor: i64 = 0;

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
        .height(40.0)
        .background(colors::BG_TRACK)
        .border(1.0, colors::BORDER, 0.0)
        .padding(4.0)
        .align_items(repose_core::AlignItems::Center)
        .on_drag_over({
            move |event: DragOver| {
                let drag_frame = (event.position.x / px_per_frame).round() as i64;
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
            move |event: DropEvent| {
                let drop_frame = (event.position.x / px_per_frame).round() as i64;
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

fn clip_view(
    store: Rc<Store>,
    clip: &Clip,
    track_type: TrackType,
    px_per_frame: f32,
    selected_clip: Option<ClipId>,
) -> View {
    let dur = clip.effective_duration().max(1);
    let w = dur as f32 * px_per_frame;

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
    let waveform = if track_type == TrackType::Audio {
        audio_waveform_placeholder()
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
        .width(w)
        .height(32.0)
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
        Box(Modifier::new().flex_grow(1.0).padding(4.0)).child(
            Column(Modifier::new().fill_max_width())
                .child((Text(label).size(10.0).color(colors::TEXT_PRIMARY), waveform)),
        ),
        right_handle,
    ));

    Button(clip_content, {
        move || {
            store_for_select.state.selected_clip_id.set(Some(clip_id));
            store_for_select.state.selected_asset_id.set(None);
        }
    })
}

fn frames_to_timecode(frames: i64, fps: i64) -> String {
    if fps <= 0 {
        return "00:00:00:00".to_string();
    }
    let frames_per_hour = fps * 60 * 60;
    let frames_per_min = fps * 60;

    let hours = frames / frames_per_hour;
    let minutes = (frames % frames_per_hour) / frames_per_min;
    let seconds = (frames % frames_per_min) / fps;
    let frame_num = frames % fps;

    format!(
        "{:02}:{:02}:{:02}:{:02}",
        hours, minutes, seconds, frame_num
    )
}
