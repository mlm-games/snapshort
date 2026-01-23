//! Panel definitions for the docking system
//!
//! This module defines all dockable panels and their content factories.

use crate::state::Store;
use repose_core::{Color, Modifier, View};
use repose_docking::{DockKind, DockNode, DockPanel, DockState, PanelId, SplitDir};
use repose_ui::{Box, Column, Row, Slider, Text, TextStyle, ViewExt};
use snapshort_ui_core::colors;
use std::rc::Rc;

// Panel IDs - unique identifiers for each dockable panel
pub const PANEL_PROJECT: PanelId = 1;
pub const PANEL_MEDIA_BROWSER: PanelId = 2;
pub const PANEL_EFFECTS: PanelId = 3;
pub const PANEL_PROGRAM_MONITOR: PanelId = 4;
pub const PANEL_SOURCE_MONITOR: PanelId = 5;
pub const PANEL_TIMELINE: PanelId = 6;
pub const PANEL_INSPECTOR: PanelId = 7;
pub const PANEL_HISTORY: PanelId = 8;
pub const PANEL_AUDIO_MIXER: PanelId = 9;

/// Create all dockable panels with their content factories
pub fn create_panels(store: Rc<Store>) -> Vec<DockPanel> {
    vec![
        DockPanel {
            id: PANEL_PROJECT,
            title: "Project".into(),
            content: {
                let store = store.clone();
                Rc::new(move || super::assets::assets_panel(store.clone()))
            },
        },
        DockPanel {
            id: PANEL_MEDIA_BROWSER,
            title: "Media Browser".into(),
            content: Rc::new(|| media_browser_content()),
        },
        DockPanel {
            id: PANEL_EFFECTS,
            title: "Effects".into(),
            content: Rc::new(|| effects_content()),
        },
        DockPanel {
            id: PANEL_PROGRAM_MONITOR,
            title: "Program".into(),
            content: {
                let store = store.clone();
                Rc::new(move || program_monitor_content(store.clone()))
            },
        },
        DockPanel {
            id: PANEL_SOURCE_MONITOR,
            title: "Source".into(),
            content: Rc::new(|| source_monitor_content()),
        },
        DockPanel {
            id: PANEL_TIMELINE,
            title: "Timeline".into(),
            content: {
                let store = store.clone();
                Rc::new(move || super::timeline::timeline_panel(store.clone()))
            },
        },
        DockPanel {
            id: PANEL_INSPECTOR,
            title: "Inspector".into(),
            content: {
                let store = store.clone();
                Rc::new(move || inspector_panel_content(store.clone()))
            },
        },
        DockPanel {
            id: PANEL_HISTORY,
            title: "History".into(),
            content: Rc::new(|| history_content()),
        },
        DockPanel {
            id: PANEL_AUDIO_MIXER,
            title: "Audio Mixer".into(),
            content: Rc::new(|| audio_mixer_content()),
        },
    ]
}

/// Create the default dock layout that matches the original fixed layout:
///
/// ```text
/// +------------------+-------------------------+------------------+
/// |                  |     Program Monitor     |                  |
/// |     Project      +-------------------------+    Inspector     |
/// | (Media Browser)  |     Source Monitor      |    (History)     |
/// |    (Effects)     +-------------------------+  (Audio Mixer)   |
/// |                  |        Timeline         |                  |
/// +------------------+-------------------------+------------------+
/// ```
pub fn create_default_layout() -> DockState {
    // Left panel: Project, Media Browser, Effects as tabs
    let left_tabs = DockNode {
        id: 10,
        kind: DockKind::Tabs {
            tabs: vec![PANEL_PROJECT, PANEL_MEDIA_BROWSER, PANEL_EFFECTS],
            active: Some(PANEL_PROJECT),
        },
    };

    // Right panel: Inspector, History, Audio Mixer as tabs
    let right_tabs = DockNode {
        id: 11,
        kind: DockKind::Tabs {
            tabs: vec![PANEL_INSPECTOR, PANEL_HISTORY, PANEL_AUDIO_MIXER],
            active: Some(PANEL_INSPECTOR),
        },
    };

    // Program monitor
    let program_monitor = DockNode {
        id: 12,
        kind: DockKind::Tabs {
            tabs: vec![PANEL_PROGRAM_MONITOR],
            active: Some(PANEL_PROGRAM_MONITOR),
        },
    };

    // Source monitor
    let source_monitor = DockNode {
        id: 13,
        kind: DockKind::Tabs {
            tabs: vec![PANEL_SOURCE_MONITOR],
            active: Some(PANEL_SOURCE_MONITOR),
        },
    };

    // Timeline
    let timeline = DockNode {
        id: 14,
        kind: DockKind::Tabs {
            tabs: vec![PANEL_TIMELINE],
            active: Some(PANEL_TIMELINE),
        },
    };

    // Monitors split vertically (program on top, source below)
    let monitors_split = DockNode {
        id: 15,
        kind: DockKind::Split {
            dir: SplitDir::Vertical,
            ratio: 0.55,
            a: std::boxed::Box::new(program_monitor),
            b: std::boxed::Box::new(source_monitor),
        },
    };

    // Center area: monitors on top, timeline on bottom
    let center_split = DockNode {
        id: 16,
        kind: DockKind::Split {
            dir: SplitDir::Vertical,
            ratio: 0.6,
            a: std::boxed::Box::new(monitors_split),
            b: std::boxed::Box::new(timeline),
        },
    };

    // Center + right panel split
    let center_right_split = DockNode {
        id: 17,
        kind: DockKind::Split {
            dir: SplitDir::Horizontal,
            ratio: 0.8,
            a: std::boxed::Box::new(center_split),
            b: std::boxed::Box::new(right_tabs),
        },
    };

    // Root: left + (center + right)
    let root = DockNode {
        id: 1,
        kind: DockKind::Split {
            dir: SplitDir::Horizontal,
            ratio: 0.18,
            a: std::boxed::Box::new(left_tabs),
            b: std::boxed::Box::new(center_right_split),
        },
    };

    DockState::from_root(root, 17) // 17 is the highest node ID used in the layout
}

// ============================================================================
// Panel Content Functions
// ============================================================================

fn program_monitor_content(store: Rc<Store>) -> View {
    use snapshort_domain::Frame;
    use snapshort_ui_core::icon_button;
    use snapshort_usecases::{PlaybackCommand, TimelineCommand};

    let playhead = store
        .state
        .timeline
        .get()
        .map(|t| t.playhead.0)
        .unwrap_or(0);

    let store_for_undo = store.clone();
    let store_for_redo = store.clone();

    // Toolbar
    let toolbar = Row(Modifier::new()
        .fill_max_width()
        .height(32.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(8.0)
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        icon_button("↶", {
            let store = store_for_undo.clone();
            move || store.dispatch_timeline(TimelineCommand::Undo)
        }),
        h_spacer(4.0),
        icon_button("↷", {
            let store = store_for_redo.clone();
            move || store.dispatch_timeline(TimelineCommand::Redo)
        }),
        h_spacer(8.0),
        Box(Modifier::new()
            .width(1.0)
            .height(16.0)
            .background(colors::BORDER)),
        h_spacer(8.0),
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
    ]);

    // Video preview area
    let preview = Box(Modifier::new()
        .fill_max_width()
        .flex_grow(1.0)
        .padding(16.0)
        .background(Color::BLACK))
    .child(
        Box(Modifier::new()
            .fill_max_size()
            .background(colors::BG_DARK)
            .border(1.0, colors::BORDER, 0.0))
        .child(Text("No Video").size(14.0).color(colors::TEXT_MUTED)),
    );

    // Playback controls
    let controls = Row(Modifier::new()
        .fill_max_width()
        .height(40.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(8.0)
        .justify_content(repose_core::JustifyContent::Center))
    .child(vec![
        playback_button(
            store.clone(),
            "⏮",
            PlaybackCommand::Seek { frame: Frame(0) },
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
    ]);

    Column(Modifier::new().fill_max_size().background(colors::BG_DARK))
        .child((toolbar, preview, controls))
}

fn source_monitor_content() -> View {
    Column(Modifier::new().fill_max_size().background(colors::BG_DARK)).child((Box(Modifier::new(
    )
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
    ),))
}

fn inspector_panel_content(store: Rc<Store>) -> View {
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

            let track_label = match clip.track.track_type {
                snapshort_domain::TrackType::Video => format!("V{}", clip.track.index + 1),
                snapshort_domain::TrackType::Audio => format!("A{}", clip.track.index + 1),
            };

            let speed = clip.effects.speed;
            let opacity = clip.effects.opacity;
            let store_for_speed = store.clone();
            let store_for_opacity = store.clone();

            let info_section = Column(Modifier::new().fill_max_width()).child((
                Text("Selected Clip").size(12.0).color(colors::TEXT_PRIMARY),
                v_spacer(8.0),
                kv("Clip ID", format!("{}", clip.id.0)),
                kv("Asset", asset_name),
                kv("Track", track_label),
                kv("Start (frame)", format!("{}", clip.timeline_start.0)),
                kv("End (frame)", format!("{}", clip.timeline_end().0)),
                kv(
                    "Duration (frames)",
                    format!("{}", clip.effective_duration()),
                ),
            ));

            let effects_section = Column(Modifier::new().fill_max_width()).child((
                v_spacer(10.0),
                Text("Clip Properties").size(11.0).color(colors::TEXT_MUTED),
                property_slider(
                    "Speed",
                    speed,
                    (0.25, 4.0),
                    format!("{:.2}x", speed),
                    move |value| {
                        store_for_speed.dispatch_timeline(
                            snapshort_usecases::TimelineCommand::SetClipSpeed {
                                clip_id,
                                speed: value,
                            },
                        );
                    },
                ),
                property_slider(
                    "Opacity",
                    opacity,
                    (0.0, 1.0),
                    format!("{:.0}%", opacity * 100.0),
                    move |value| {
                        store_for_opacity.dispatch_timeline(
                            snapshort_usecases::TimelineCommand::SetClipOpacity {
                                clip_id,
                                opacity: value,
                            },
                        );
                    },
                ),
            ));

            return Column(Modifier::new().fill_max_size().padding(8.0))
                .child((info_section, effects_section));
        }
    }

    if let Some(asset_id) = selected_asset_id {
        if let Some(a) = assets.iter().find(|a| a.id == asset_id) {
            let path = a.path.to_string_lossy().to_string();
            let status = format!("{:?}", a.status);
            let dur = a.media_info.as_ref().map(|m| m.duration_ms).unwrap_or(0);

            return Column(Modifier::new().fill_max_size().padding(8.0)).child((
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

    Column(Modifier::new().fill_max_size().padding(8.0)).child((
        Text("Inspector").size(12.0).color(colors::TEXT_MUTED),
        v_spacer(6.0),
        Text("Click an asset or a clip to inspect it.")
            .size(11.0)
            .color(colors::TEXT_DISABLED),
    ))
}

fn history_content() -> View {
    Column(Modifier::new().fill_max_size().padding(8.0)).child((
        Text("History").size(12.0).color(colors::TEXT_MUTED),
        v_spacer(8.0),
        Text("Project Created")
            .size(11.0)
            .color(colors::TEXT_PRIMARY),
        Text("Timeline Created")
            .size(11.0)
            .color(colors::TEXT_PRIMARY),
    ))
}

fn media_browser_content() -> View {
    Column(Modifier::new().fill_max_size().padding(8.0)).child((
        Text("Media Browser").size(12.0).color(colors::TEXT_MUTED),
        v_spacer(8.0),
        Text("Browse your media files here.")
            .size(11.0)
            .color(colors::TEXT_DISABLED),
    ))
}

fn effects_content() -> View {
    Column(Modifier::new().fill_max_size().padding(8.0)).child(vec![
        Text("Effects").size(12.0).color(colors::TEXT_MUTED),
        v_spacer(8.0),
        Text("Video Effects").size(11.0).color(colors::TEXT_PRIMARY),
        Text("  Color Correction")
            .size(10.0)
            .color(colors::TEXT_DISABLED),
        Text("  Blur").size(10.0).color(colors::TEXT_DISABLED),
        Text("  Sharpen").size(10.0).color(colors::TEXT_DISABLED),
        v_spacer(8.0),
        Text("Audio Effects").size(11.0).color(colors::TEXT_PRIMARY),
        Text("  EQ").size(10.0).color(colors::TEXT_DISABLED),
        Text("  Compressor").size(10.0).color(colors::TEXT_DISABLED),
        Text("  Reverb").size(10.0).color(colors::TEXT_DISABLED),
    ])
}

fn audio_mixer_content() -> View {
    Column(Modifier::new().fill_max_size().padding(8.0)).child((
        Text("Audio Mixer").size(12.0).color(colors::TEXT_MUTED),
        v_spacer(8.0),
        Row(Modifier::new()
            .fill_max_width()
            .align_items(repose_core::AlignItems::End))
        .child((
            audio_channel("A1", 0.8),
            h_spacer(8.0),
            audio_channel("A2", 0.6),
            h_spacer(8.0),
            audio_channel("Master", 0.75),
        )),
    ))
}

// ============================================================================
// Helper Functions
// ============================================================================

fn h_spacer(w: f32) -> View {
    Box(Modifier::new().width(w))
}

fn v_spacer(h: f32) -> View {
    Box(Modifier::new().height(h))
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

fn property_slider(
    label: &str,
    value: f32,
    range: (f32, f32),
    value_label: String,
    on_change: impl Fn(f32) + 'static,
) -> View {
    Row(Modifier::new()
        .fill_max_width()
        .height(28.0)
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        Text(label).size(11.0).color(colors::TEXT_MUTED),
        Box(Modifier::new().width(8.0)),
        Slider(value, range, None, on_change).modifier(Modifier::new().width(120.0).height(18.0)),
        Box(Modifier::new().flex_grow(1.0)),
        Text(value_label).size(11.0).color(colors::TEXT_PRIMARY),
    ])
}

fn playback_button(store: Rc<Store>, icon: &str, cmd: snapshort_usecases::PlaybackCommand) -> View {
    snapshort_ui_core::icon_button(icon, move || store.dispatch_playback(cmd.clone()))
        .modifier(Modifier::new().width(32.0).height(32.0))
}

fn playback_seek_rel(store: Rc<Store>, icon: &str, delta: i64) -> View {
    snapshort_ui_core::icon_button(icon, move || {
        let cur = store
            .state
            .timeline
            .get()
            .map(|t| t.playhead.0)
            .unwrap_or(0);
        store.dispatch_playback(snapshort_usecases::PlaybackCommand::Seek {
            frame: snapshort_domain::Frame((cur + delta).max(0)),
        });
    })
    .modifier(Modifier::new().width(32.0).height(32.0))
}

fn audio_channel(name: &str, level: f32) -> View {
    let bar_height = 100.0 * level;

    Column(
        Modifier::new()
            .width(40.0)
            .align_items(repose_core::AlignItems::Center),
    )
    .child((
        Box(Modifier::new()
            .width(20.0)
            .height(100.0)
            .background(colors::BG_LIGHT)
            .border(1.0, colors::BORDER, 2.0))
        .child(
            Column(
                Modifier::new()
                    .fill_max_size()
                    .justify_content(repose_core::JustifyContent::End),
            )
            .child((Box(Modifier::new()
                .fill_max_width()
                .height(bar_height)
                .background(colors::AUDIO_TRACK)),)),
        ),
        v_spacer(4.0),
        Text(name).size(10.0).color(colors::TEXT_MUTED),
    ))
}
