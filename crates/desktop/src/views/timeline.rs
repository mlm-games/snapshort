use crate::state::Store;
use repose_core::{view::View, Color, Modifier};
use repose_ui::{
    scroll::{remember_scroll_state, ScrollArea},
    Box, Column, Row, Text, TextStyle, ViewExt,
};

use snapshort_domain::{Clip, ClipType, Frame, Timeline};
use snapshort_ui_core::colors;
use snapshort_usecases::TimelineCommand;

use std::rc::Rc;

const PX_PER_FRAME: f32 = 2.0; // fast + simple visual scale

fn h_spacer(w: f32) -> View {
    Box(Modifier::new().width(w))
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

    // Track header views
    let mut track_header_views: Vec<View> = Vec::new();
    track_header_views.push(Box(Modifier::new()
        .fill_max_width()
        .height(24.0)
        .background(colors::BG_PANEL)));

    for i in 0..video_track_count {
        track_header_views.push(track_header(&format!("V{}", i + 1), TrackType::Video, i));
    }
    for i in 0..audio_track_count {
        track_header_views.push(track_header(
            &format!("A{}", i + 1),
            TrackType::Audio,
            video_track_count + i, // offset to avoid key collision
        ));
    }

    track_header_views.push(track_add_buttons(store.clone()));

    // Track content views
    let mut track_content_views: Vec<View> = Vec::new();
    track_content_views.push(time_ruler());

    if let Some(tl) = &timeline {
        // video tracks (absolute indices 0..)
        for i in 0..tl.video_tracks.len() {
            track_content_views.push(track_lane(tl, i, TrackType::Video));
        }
        // audio tracks (absolute indices offset by video tracks)
        let audio_offset = tl.video_tracks.len();
        for i in 0..tl.audio_tracks.len() {
            track_content_views.push(track_lane(tl, audio_offset + i, TrackType::Audio));
        }
    } else {
        // placeholder lanes
        track_content_views.push(empty_lane(TrackType::Video));
        track_content_views.push(empty_lane(TrackType::Audio));
    }

    let header = Row(Modifier::new()
        .fill_max_width()
        .height(28.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(8.0)
        .align_items(repose_core::AlignItems::Center))
    .child((
        Text(name).size(12.0).color(colors::TEXT_PRIMARY),
        Box(Modifier::new().flex_grow(1.0)),
        Text(timecode).size(11.0).color(colors::TEXT_PRIMARY),
    ));

    let track_headers = Column(Modifier::new().width(180.0).fill_max_height().border(
        1.0,
        colors::BORDER,
        0.0,
    ))
    .child(track_header_views);

    let track_content = Column(Modifier::new().fill_max_width().flex_grow(1.0)).child(ScrollArea(
        Modifier::new().fill_max_size(),
        remember_scroll_state("timeline_tracks"),
        Column(Modifier::new().fill_max_width()).child(track_content_views),
    ));

    let main_content =
        Row(Modifier::new().fill_max_size().flex_grow(1.0)).child((track_headers, track_content));

    Column(
        Modifier::new()
            .fill_max_size()
            .height(350.0)
            .background(colors::BG_DARK),
    )
    .child((header, main_content))
}

#[derive(Clone, Copy)]
enum TrackType {
    Video,
    Audio,
}

impl TrackType {
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
}

fn track_header(name: &str, track_type: TrackType, index: usize) -> View {
    Row(Modifier::new()
        .key(index as u64)
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
        // Add video track
        snapshort_ui_core::icon_button("+V", {
            let store = store.clone();
            move || store.dispatch_timeline(TimelineCommand::AddVideoTrack)
        }),
        h_spacer(8.0),
        // Add audio track
        snapshort_ui_core::icon_button("+A", {
            let store = store.clone();
            move || store.dispatch_timeline(TimelineCommand::AddAudioTrack)
        }),
        Box(Modifier::new().flex_grow(1.0)),
        Text("Add Track").size(10.0).color(colors::TEXT_MUTED),
    ))
}

fn time_ruler() -> View {
    Row(Modifier::new()
        .fill_max_width()
        .height(28.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(6.0)
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        time_marker("00:00:00:00"),
        h_spacer(24.0),
        time_marker("00:00:01:00"),
        h_spacer(24.0),
        time_marker("00:00:02:00"),
        h_spacer(24.0),
        time_marker("00:00:03:00"),
        h_spacer(24.0),
        time_marker("00:00:04:00"),
    ])
}

fn time_marker(label: &str) -> View {
    Column(Modifier::new().align_items(repose_core::AlignItems::Center)).child((
        Box(Modifier::new()
            .width(1.0)
            .height(6.0)
            .background(colors::TEXT_MUTED)),
        Text(label).size(10.0).color(colors::TEXT_MUTED),
    ))
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

fn track_lane(timeline: &Timeline, track_index: usize, track_type: TrackType) -> View {
    let mut clips: Vec<Clip> = timeline.clips_on_track(track_index).cloned().collect();
    clips.sort_by_key(|c| c.timeline_start.0);

    let mut children: Vec<View> = Vec::new();
    let mut cursor: i64 = 0;

    for clip in clips.iter() {
        let start = clip.timeline_start.0;
        let gap_frames = (start - cursor).max(0);
        if gap_frames > 0 {
            children.push(Box(Modifier::new().width(gap_frames as f32 * PX_PER_FRAME)));
        }

        let dur = clip.effective_duration().max(1);
        let w = dur as f32 * PX_PER_FRAME;

        let (bg, border, label) = match clip.clip_type {
            ClipType::Gap => (colors::BG_LIGHT, colors::BORDER, "Gap".to_string()),
            _ => (
                track_type.bg_color(),
                track_type.color(),
                clip.name.clone().unwrap_or_else(|| "Clip".to_string()),
            ),
        };

        children.push(
            Box(Modifier::new()
                .width(w)
                .height(32.0)
                .background(bg)
                .border(1.0, border, 0.0)
                .padding(4.0))
            .child(Text(label).size(10.0).color(colors::TEXT_PRIMARY)),
        );

        children.push(Box(Modifier::new().width(4.0))); // spacing
        cursor = clip.timeline_end().0;
    }

    children.push(Box(Modifier::new().flex_grow(1.0)));

    Row(Modifier::new()
        .fill_max_width()
        .height(40.0)
        .background(colors::BG_TRACK)
        .border(1.0, colors::BORDER, 0.0)
        .padding(4.0)
        .align_items(repose_core::AlignItems::Center))
    .child(children)
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
