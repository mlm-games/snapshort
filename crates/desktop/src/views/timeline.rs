use crate::state::Store;
use repose_core::{Color, Modifier, View};
use repose_ui::scroll::{remember_scroll_state, ScrollArea};
use repose_ui::{Box, Column, Row, Text, TextStyle, ViewExt};
use snapshort_ui_core::colors;
use std::rc::Rc;

fn h_spacer(w: f32) -> View {
    Box(Modifier::new().width(w))
}

pub fn timeline_panel(store: Rc<Store>) -> View {
    let timeline = store.state.timeline.get();
    let name = timeline
        .as_ref()
        .map(|t| t.name.clone())
        .unwrap_or("No Timeline".into());

    let video_track_count = timeline.as_ref().map(|t| t.video_tracks.len()).unwrap_or(1);
    let audio_track_count = timeline.as_ref().map(|t| t.audio_tracks.len()).unwrap_or(1);

    // Calculate total duration in frames (dummy for now)
    let total_frames = timeline.as_ref().map(|t| t.duration().0).unwrap_or(0);
    let timecode = frames_to_timecode(total_frames, 24);

    // Build track header views (variable sized => Vec<View>)
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
            video_track_count + i,
        ));
    }

    track_header_views.push(track_add_button());

    // Build track content views (variable sized => Vec<View>)
    let mut track_content_views: Vec<View> = Vec::new();
    for i in 0..video_track_count {
        track_content_views.push(video_track_content(i));
    }
    for i in 0..audio_track_count {
        track_content_views.push(audio_track_content(i));
    }

    Column(
        Modifier::new()
            .fill_max_size()
            .height(350.0)
            .background(colors::BG_DARK),
    )
    // Timeline Header
    .child(
        Row(Modifier::new()
            .fill_max_width()
            .height(28.0)
            .background(colors::BG_PANEL)
            .border(1.0, colors::BORDER, 0.0)
            .padding(8.0)
            .align_items(repose_core::AlignItems::Center))
        .child(
            Text(name).size(12.0).color(colors::TEXT_PRIMARY), // .font_weight(TextStyle::bold()),
        )
        .child(Box(Modifier::new().flex_grow(1.0)))
        .child(
            Text(timecode).size(11.0).color(colors::TEXT_PRIMARY), // .font_weight(TextStyle::bold()),
        )
        .child(h_spacer(8.0))
        .child(Box(Modifier::new()
            .width(1.0)
            .height(16.0)
            .background(colors::BORDER)))
        .child(h_spacer(8.0))
        .child(Text("Zoom").size(11.0).color(colors::TEXT_MUTED))
        .child(h_spacer(8.0))
        .child(
            Box(Modifier::new()
                .width(80.0)
                .height(20.0)
                .background(colors::BG_DARK)
                .border(1.0, colors::BORDER, 2.0)
                .padding(2.0))
            .child(Text("100%").size(10.0).color(colors::TEXT_PRIMARY)),
        ),
    )
    // Main timeline content
    .child(
        Row(Modifier::new().fill_max_size().flex_grow(1.0))
            // Track Headers (Left side)
            .child(
                Column(Modifier::new().width(180.0).fill_max_height().border(
                    1.0,
                    colors::BORDER,
                    0.0,
                ))
                .child(track_header_views),
            )
            // Timeline Tracks (Right side - scrollable)
            .child(
                Column(Modifier::new().fill_max_width().flex_grow(1.0))
                    // Time ruler
                    .child(time_ruler())
                    // Track content
                    .child(ScrollArea(
                        Modifier::new().fill_max_size(),
                        remember_scroll_state("timeline_tracks"),
                        Column(Modifier::new().fill_max_width()).child(track_content_views),
                    )),
            ),
    )
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
            TrackType::Video => Color::from_rgb(0x1E, 0x3A, 0x5F), // Dark blue
            TrackType::Audio => Color::from_rgb(0x2D, 0x5A, 0x27), // Dark green
        }
    }
}

fn track_header(name: &str, track_type: TrackType, index: usize) -> View {
    Row(Modifier::new()
        .key(index as u64)
        .fill_max_width()
        .height(40.0)
        .border(1.0, colors::BORDER, 0.0)
        .padding(8.0)
        .align_items(repose_core::AlignItems::Center))
    .child(Box(Modifier::new().width(12.0).height(12.0).border(
        1.0,
        track_type.color(),
        0.0,
    )))
    .child(h_spacer(6.0))
    .child(
        Text(name).size(11.0).color(colors::TEXT_PRIMARY), // .font_weight(TextStyle::bold()),
    )
    .child(Box(Modifier::new().flex_grow(1.0)))
    .child(
        Row(Modifier::new())
            .child(track_header_icon("👁"))
            .child(h_spacer(4.0))
            .child(track_header_icon("🔒"))
            .child(h_spacer(4.0))
            .child(track_header_icon("M")),
    )
    .child(h_spacer(4.0))
    .child(
        Box(Modifier::new()
            .width(24.0)
            .height(20.0)
            .background(track_type.bg_color())
            .border(1.0, track_type.color(), 0.0))
        .child(Text("3").size(10.0).color(colors::TEXT_PRIMARY)),
    )
}

fn track_header_icon(icon: &str) -> View {
    Box(Modifier::new()
        .width(16.0)
        .height(16.0)
        .on_pointer_enter(|_| {}))
    .child(Text(icon).size(10.0).color(colors::TEXT_MUTED))
}

fn track_add_button() -> View {
    Box(Modifier::new()
        .fill_max_width()
        .height(32.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(8.0)
        .on_pointer_enter(|_| {}))
    .child(
        Row(Modifier::new().align_items(repose_core::AlignItems::Center))
            .child(Text("+").size(14.0).color(colors::TEXT_ACCENT))
            .child(Text(" Add Track").size(11.0).color(colors::TEXT_MUTED)),
    )
}

fn time_ruler() -> View {
    Row(Modifier::new()
        .fill_max_width()
        .height(24.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0))
    .child(time_marker("00:00:00:00"))
    .child(Box(Modifier::new().flex_grow(1.0)))
    .child(time_marker("00:00:01:00"))
    .child(Box(Modifier::new().flex_grow(1.0)))
    .child(time_marker("00:00:02:00"))
    .child(Box(Modifier::new().flex_grow(1.0)))
    .child(time_marker("00:00:03:00"))
    .child(Box(Modifier::new().flex_grow(1.0)))
    .child(time_marker("00:00:04:00"))
}

fn time_marker(label: &str) -> View {
    Column(Modifier::new().align_items(repose_core::AlignItems::Center))
        .child(Box(Modifier::new()
            .width(1.0)
            .height(6.0)
            .background(colors::TEXT_MUTED)))
        .child(Text(label).size(10.0).color(colors::TEXT_MUTED))
}

fn video_track_content(track_index: usize) -> View {
    Box(Modifier::new()
        .fill_max_width()
        .height(40.0)
        .background(colors::BG_TRACK)
        .border(1.0, colors::BORDER, 0.0)
        .padding(4.0))
    .child(
        Row(Modifier::new().fill_max_size())
            .child(h_spacer(50.0))
            // Playhead indicator
            .child(Box(Modifier::new()
                .width(2.0)
                .fill_max_height()
                .background(Color::from_rgb(0xFF, 0x6B, 0x6B))))
            .child(h_spacer(20.0))
            // Video clip placeholder
            .child(
                Box(Modifier::new()
                    .width(200.0)
                    .height(32.0)
                    .background(Color::from_rgb(0x4A, 0x90, 0xE2))
                    .border(1.0, Color::from_rgb(0x21, 0x71, 0xB5), 0.0)
                    .padding(4.0))
                .child(
                    Row(Modifier::new().align_items(repose_core::AlignItems::Center))
                        .child(Text("V").size(11.0).color(Color::WHITE))
                        .child(
                            Text(format!(" Track {} Clip", track_index + 1))
                                .size(11.0)
                                .color(Color::WHITE),
                        ),
                ),
            )
            .child(h_spacer(8.0))
            // Another clip
            .child(
                Box(Modifier::new()
                    .width(150.0)
                    .height(32.0)
                    .background(Color::from_rgb(0x5D, 0xAD, 0xE2))
                    .border(1.0, Color::from_rgb(0x28, 0x74, 0xA6), 0.0)
                    .padding(4.0))
                .child(Text("Transition").size(11.0).color(Color::WHITE)),
            ),
    )
}

fn audio_track_content(track_index: usize) -> View {
    Box(Modifier::new()
        .fill_max_width()
        .height(40.0)
        .background(colors::BG_TRACK)
        .border(1.0, colors::BORDER, 0.0)
        .padding(4.0))
    .child(
        Row(Modifier::new().fill_max_size())
            .child(h_spacer(50.0))
            // Playhead indicator
            .child(Box(Modifier::new()
                .width(2.0)
                .fill_max_height()
                .background(Color::from_rgb(0xFF, 0x6B, 0x6B))))
            .child(h_spacer(20.0))
            // Audio clip placeholder
            .child(
                Box(Modifier::new()
                    .width(180.0)
                    .height(32.0)
                    .background(Color::from_rgb(0x52, 0xBE, 0x80))
                    .border(1.0, Color::from_rgb(0x28, 0xB4, 0x63), 0.0)
                    .padding(4.0))
                .child(
                    Row(Modifier::new().align_items(repose_core::AlignItems::Center))
                        .child(Text("A").size(11.0).color(Color::WHITE))
                        .child(
                            Text(format!(" Track {} Audio", track_index + 1))
                                .size(11.0)
                                .color(Color::WHITE),
                        ),
                ),
            )
            .child(h_spacer(8.0))
            // Waveform visualization placeholder
            .child(
                Box(Modifier::new()
                    .width(100.0)
                    .height(32.0)
                    .background(Color::from_rgb(0x58, 0xD6, 0x8D))
                    .border(1.0, Color::from_rgb(0x1D, 0x83, 0x48), 0.0))
                .child(Text("🎵 Music").size(11.0).color(Color::WHITE)),
            ),
    )
}

fn frames_to_timecode(frames: i64, fps: i64) -> String {
    let total_seconds = frames / fps;
    let frames = frames % fps;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    format!("{:02}:{:02}:{:02}:{:02}", hours, minutes, seconds, frames)
}
