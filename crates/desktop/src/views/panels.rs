//! Panel definitions for the docking system

use crate::state::Store;
use repose_core::prelude::theme;
use repose_core::{Color, Modifier, View};
use repose_docking::{DockKind, DockNode, DockPanel, DockState, PanelId, SplitDir};
use repose_material::material3;
use repose_ui::{Box, Column, Image, ImageExt, Row, Slider, Text, TextStyle, ViewExt};
use snapshort_infra_render::{OutputFormat, QualityPreset};
use snapshort_usecases::ProjectCommand;
use std::rc::Rc;

// Panel IDs
pub const PANEL_PROJECT: PanelId = 1;
pub const PANEL_MEDIA_BROWSER: PanelId = 2;
pub const PANEL_EFFECTS: PanelId = 3;
pub const PANEL_PROGRAM_MONITOR: PanelId = 4;
pub const PANEL_SOURCE_MONITOR: PanelId = 5;
pub const PANEL_TIMELINE: PanelId = 6;
pub const PANEL_INSPECTOR: PanelId = 7;
pub const PANEL_HISTORY: PanelId = 8;
pub const PANEL_AUDIO_MIXER: PanelId = 9;
pub const PANEL_EXPORT: PanelId = 10;

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
            content: {
                let store = store.clone();
                Rc::new(move || audio_mixer_content(store.clone()))
            },
        },
        DockPanel {
            id: PANEL_EXPORT,
            title: "Export".into(),
            content: {
                let store = store.clone();
                Rc::new(move || export_panel_content(store.clone()))
            },
        },
    ]
}

/// Default dock layout
pub fn create_default_layout() -> DockState {
    let left_tabs = DockNode {
        id: 10,
        kind: DockKind::Tabs {
            tabs: vec![PANEL_PROJECT, PANEL_MEDIA_BROWSER, PANEL_EFFECTS],
            active: Some(PANEL_PROJECT),
        },
    };

    let right_tabs = DockNode {
        id: 11,
        kind: DockKind::Tabs {
            tabs: vec![
                PANEL_INSPECTOR,
                PANEL_HISTORY,
                PANEL_AUDIO_MIXER,
                PANEL_EXPORT,
            ],
            active: Some(PANEL_INSPECTOR),
        },
    };

    let program_monitor = DockNode {
        id: 12,
        kind: DockKind::Tabs {
            tabs: vec![PANEL_PROGRAM_MONITOR, PANEL_SOURCE_MONITOR],
            active: Some(PANEL_PROGRAM_MONITOR),
        },
    };

    let timeline = DockNode {
        id: 14,
        kind: DockKind::Tabs {
            tabs: vec![PANEL_TIMELINE],
            active: Some(PANEL_TIMELINE),
        },
    };

    let center_split = DockNode {
        id: 16,
        kind: DockKind::Split {
            dir: SplitDir::Vertical,
            ratio: 0.58,
            a: std::boxed::Box::new(program_monitor),
            b: std::boxed::Box::new(timeline),
        },
    };

    let center_right_split = DockNode {
        id: 17,
        kind: DockKind::Split {
            dir: SplitDir::Horizontal,
            ratio: 0.78,
            a: std::boxed::Box::new(center_split),
            b: std::boxed::Box::new(right_tabs),
        },
    };

    let root = DockNode {
        id: 1,
        kind: DockKind::Split {
            dir: SplitDir::Horizontal,
            ratio: 0.2,
            a: std::boxed::Box::new(left_tabs),
            b: std::boxed::Box::new(center_right_split),
        },
    };

    DockState::from_root(root, 17)
}

fn program_monitor_content(store: Rc<Store>) -> View {
    use snapshort_domain::Frame;
    use snapshort_usecases::{PlaybackCommand, PreviewCommand, TimelineCommand};

    let th = theme();

    let playhead = store
        .state
        .timeline
        .get()
        .map(|t| t.playhead.0)
        .unwrap_or(0);

    let fps = store
        .state
        .timeline
        .get()
        .map(|t| t.settings.fps.as_f64())
        .unwrap_or(24.0);

    let store_for_undo = store.clone();
    let store_for_redo = store.clone();
    let last_render_plan = store.state.last_render_plan_summary.get();
    let preview_handle = store.state.preview_image_handle.get();
    let playback_state = store.state.playback_state.get();
    store.dispatch_preview(PreviewCommand::RequestFrame {
        frame: Frame(playhead),
    });

    let zoom_percent = (store.state.timeline_zoom.get() / 2.0 * 100.0).round() as i32;

    let toolbar = Row(Modifier::new()
        .fill_max_width()
        .height(40.0)
        .background(th.surface)
        .border(1.0, th.outline, 0.0)
        .padding_values(repose_core::PaddingValues {
            left: 10.0,
            right: 10.0,
            top: 6.0,
            bottom: 6.0,
        })
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        material3::IconButton(Text("↶").size(18.0), {
            let store = store_for_undo.clone();
            move || store.dispatch_timeline(TimelineCommand::Undo)
        }),
        h_spacer(6.0),
        material3::IconButton(Text("↷").size(18.0), {
            let store = store_for_redo.clone();
            move || store.dispatch_timeline(TimelineCommand::Redo)
        }),
        h_spacer(10.0),
        Box(Modifier::new()
            .width(1.0)
            .height(16.0)
            .background(th.outline.with_alpha(128))),
        h_spacer(10.0),
        Text(format!("{}%", zoom_percent))
            .size(11.0)
            .color(th.on_surface),
        Box(Modifier::new().flex_grow(1.0)),
        Text(format!("{:.0}fps", fps))
            .size(11.0)
            .color(th.on_surface_variant),
        h_spacer(14.0),
        Box(Modifier::new()
            .width(1.0)
            .height(16.0)
            .background(th.outline.with_alpha(128))),
        h_spacer(14.0),
        Text(format!("Frame: {playhead}"))
            .size(11.0)
            .color(th.on_surface),
    ]);

    let preview = Box(Modifier::new()
        .fill_max_width()
        .flex_grow(1.0)
        .padding(12.0)
        .background(Color::BLACK))
    .child(
        Column(Modifier::new().fill_max_size()).child((
            Box(Modifier::new()
                .fill_max_width()
                .flex_grow(1.0)
                .background(th.background)
                .border(1.0, th.outline, 8.0)
                .clip_rounded(8.0)
                .align_items(repose_core::AlignItems::Center)
                .justify_content(repose_core::JustifyContent::Center))
            .child(
                Image(
                    Modifier::new()
                        .fill_max_width()
                        .flex_grow(1.0)
                        .aspect_ratio(16.0 / 9.0),
                    preview_handle,
                )
                .image_fit(repose_core::ImageFit::Contain),
            ),
            v_spacer(8.0),
            Row(Modifier::new()
                .fill_max_width()
                .align_items(repose_core::AlignItems::Center))
            .child((
                Text(format!("Frame: {} ({})", playhead, playback_state))
                    .size(10.0)
                    .color(th.on_surface_variant),
                Box(Modifier::new().flex_grow(1.0)),
                Text(last_render_plan.unwrap_or_else(|| "Render plan not generated".into()))
                    .size(10.0)
                    .color(th.on_surface_variant.with_alpha(160)),
            )),
        )),
    );

    let controls = Row(Modifier::new()
        .fill_max_width()
        .height(48.0)
        .background(th.surface)
        .border(1.0, th.outline, 0.0)
        .padding_values(repose_core::PaddingValues {
            left: 12.0,
            right: 12.0,
            top: 6.0,
            bottom: 6.0,
        })
        .justify_content(repose_core::JustifyContent::Center)
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        playback_button(
            store.clone(),
            "⏮",
            PlaybackCommand::Seek { frame: Frame(0) },
        ),
        h_spacer(12.0),
        playback_seek_rel(store.clone(), "◀", -24),
        h_spacer(12.0),
        playback_button(store.clone(), "▶", PlaybackCommand::Play),
        h_spacer(12.0),
        playback_button(store.clone(), "⏸", PlaybackCommand::Pause),
        h_spacer(12.0),
        playback_button(store.clone(), "⏹", PlaybackCommand::Stop),
        h_spacer(12.0),
        playback_seek_rel(store.clone(), "⏭", 24),
    ]);

    Column(Modifier::new().fill_max_size().background(th.background))
        .child((toolbar, preview, controls))
}

fn source_monitor_content() -> View {
    let th = theme();
    Column(Modifier::new().fill_max_size().background(th.background)).child((Box(Modifier::new()
        .fill_max_width()
        .flex_grow(1.0)
        .padding(16.0)
        .background(Color::BLACK))
    .child(
        Box(Modifier::new()
            .fill_max_size()
            .background(th.surface)
            .border(1.0, th.outline, 12.0)
            .clip_rounded(12.0))
        .child(Text("Source").size(12.0).color(th.on_surface_variant)),
    ),))
}

fn inspector_panel_content(store: Rc<Store>) -> View {
    let th = theme();
    let selected_clip_id = store.state.selected_clip_id.get();
    let selected_asset_id = store.state.selected_asset_id.get();
    let timeline = store.state.timeline.get();
    let assets = store.state.assets.get();

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

            let info_section = Column(Modifier::new().fill_max_width()).child((
                Text("Selected Clip").size(12.0).color(th.on_surface),
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

            return Column(Modifier::new().fill_max_size().padding(10.0)).child((info_section,));
        }
    }

    if let Some(asset_id) = selected_asset_id {
        if let Some(a) = assets.iter().find(|a| a.id == asset_id) {
            let path = a.path.to_string_lossy().to_string();
            let status = format!("{:?}", a.status);
            let dur = a.media_info.as_ref().map(|m| m.duration_ms).unwrap_or(0);

            return Column(Modifier::new().fill_max_size().padding(10.0)).child((
                Text("Selected Asset").size(12.0).color(th.on_surface),
                v_spacer(8.0),
                kv("Name", a.name.clone()),
                kv("Type", format!("{:?}", a.asset_type)),
                kv("Status", status),
                kv("Duration (ms)", format!("{dur}")),
                kv("Path", path),
            ));
        }
    }

    Column(Modifier::new().fill_max_size().padding(10.0)).child((
        Text("Inspector").size(12.0).color(th.on_surface_variant),
        v_spacer(6.0),
        Text("Click an asset or a clip to inspect it.")
            .size(11.0)
            .color(th.on_surface_variant.with_alpha(160)),
    ))
}

fn history_content() -> View {
    let th = theme();
    Column(Modifier::new().fill_max_size().padding(10.0)).child((
        Text("History").size(12.0).color(th.on_surface_variant),
        v_spacer(8.0),
        Text("Project Created").size(11.0).color(th.on_surface),
        Text("Timeline Created").size(11.0).color(th.on_surface),
    ))
}

fn media_browser_content() -> View {
    let th = theme();
    Column(Modifier::new().fill_max_size().padding(10.0)).child((
        Text("Media Browser")
            .size(12.0)
            .color(th.on_surface_variant),
        v_spacer(8.0),
        Text("Browse your media files here.")
            .size(11.0)
            .color(th.on_surface_variant.with_alpha(160)),
    ))
}

fn effects_content() -> View {
    let th = theme();
    Column(Modifier::new().fill_max_size().padding(10.0)).child(vec![
        Text("Effects").size(12.0).color(th.on_surface_variant),
        v_spacer(8.0),
        Text("Video Effects").size(11.0).color(th.on_surface),
        Text("  Color Correction")
            .size(10.0)
            .color(th.on_surface_variant),
        Text("  Blur").size(10.0).color(th.on_surface_variant),
        Text("  Sharpen").size(10.0).color(th.on_surface_variant),
        v_spacer(8.0),
        Text("Audio Effects").size(11.0).color(th.on_surface),
        Text("  EQ").size(10.0).color(th.on_surface_variant),
        Text("  Compressor").size(10.0).color(th.on_surface_variant),
        Text("  Reverb").size(10.0).color(th.on_surface_variant),
    ])
}

fn audio_mixer_content(store: Rc<Store>) -> View {
    let th = theme();
    let timeline = store.state.timeline.get();
    let audio_tracks: Vec<_> = timeline
        .as_ref()
        .map(|tl| tl.audio_tracks.iter().collect())
        .unwrap_or_default();

    let mut channels: Vec<View> = Vec::new();
    for track in audio_tracks.iter() {
        let label = format!("A{}", track.index + 1);
        channels.push(audio_channel(&label, 0.7));
        channels.push(h_spacer(8.0));
    }
    channels.push(audio_channel("Master", 0.75));

    Column(Modifier::new().fill_max_size().padding(10.0)).child((
        Text("Audio Mixer").size(12.0).color(th.on_surface_variant),
        v_spacer(8.0),
        Row(Modifier::new()
            .fill_max_width()
            .align_items(repose_core::AlignItems::End))
        .child(channels),
    ))
}

fn export_panel_content(store: Rc<Store>) -> View {
    use snapshort_usecases::RenderCommand;

    let th = theme();

    let export_path = store.state.export_output_path.get();
    let quality = store.state.export_quality.get();
    let last_result = store.state.last_render_result.get();
    let timeline = store.state.timeline.get();
    let clip_count = timeline.as_ref().map(|t| t.clips.len()).unwrap_or(0);

    let quality_label = match quality {
        QualityPreset::Draft => "Draft",
        QualityPreset::Preview => "Preview",
        QualityPreset::Standard => "Standard",
        QualityPreset::High => "High",
        QualityPreset::Master => "Master",
    };

    let header = Column(Modifier::new().fill_max_width().padding(12.0)).child((
        Text("Export").size(14.0).color(th.on_surface),
        v_spacer(4.0),
        Text(format!("Timeline clips: {}", clip_count))
            .size(11.0)
            .color(th.on_surface_variant),
    ));

    let output_row = Row(Modifier::new()
        .fill_max_width()
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        Text("Output").size(12.0).color(th.on_surface_variant),
        Box(Modifier::new().width(10.0)),
        Text(
            export_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "Not set".into()),
        )
        .size(12.0)
        .color(th.on_surface)
        .single_line(),
        Box(Modifier::new().flex_grow(1.0)),
        material3::TextButton(
            {
                let store = store.clone();
                move || {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name("export.mp4")
                        .save_file()
                    {
                        store.state.export_output_path.set(Some(path));
                    }
                }
            },
            || Text("Choose…"),
        ),
    ]);

    let export_button = material3::FilledButton(
        {
            let store = store.clone();
            move || {
                let Some(output_path) = store.state.export_output_path.get() else {
                    store.state.status_msg.set("Select an output path".into());
                    return;
                };

                store.dispatch_render(RenderCommand::Export {
                    output_path,
                    format: OutputFormat::Mp4H264,
                    quality: store.state.export_quality.get(),
                    use_hardware_accel: false,
                });
            }
        },
        move || Text("Export"),
    )
    .modifier(Modifier::new().width(160.0));

    Column(Modifier::new().fill_max_size().background(th.background)).child(vec![
        header,
        Box(Modifier::new()
            .height(1.0)
            .background(th.outline.with_alpha(128))),
        Box(Modifier::new().height(12.0)),
        output_row,
        Box(Modifier::new().height(10.0)),
        kv("Format", "MP4 H.264"),
        kv("Quality", quality_label),
        kv("Hardware accel", "Off"),
        Box(Modifier::new().height(16.0)),
        export_button,
        Box(Modifier::new().height(10.0)),
        kv("Status", last_result.unwrap_or_else(|| "Idle".into())),
    ])
}

fn h_spacer(w: f32) -> View {
    Box(Modifier::new().width(w))
}

fn v_spacer(h: f32) -> View {
    Box(Modifier::new().height(h))
}

fn kv(label: impl Into<String>, value: impl Into<String>) -> View {
    let th = theme();
    Row(Modifier::new()
        .fill_max_width()
        .height(22.0)
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        Text(label.into()).size(11.0).color(th.on_surface_variant),
        Box(Modifier::new().flex_grow(1.0)),
        Text(value.into()).size(11.0).color(th.on_surface),
    ])
}

fn playback_button(
    store: Rc<Store>,
    label: &str,
    cmd: snapshort_usecases::PlaybackCommand,
) -> View {
    material3::FilledTonalButton(
        move || store.dispatch_playback(cmd.clone()),
        move || Text(label),
    )
    .modifier(Modifier::new().height(32.0))
}

fn playback_seek_rel(store: Rc<Store>, label: &str, delta: i64) -> View {
    material3::FilledTonalButton(
        move || {
            let cur = store
                .state
                .timeline
                .get()
                .map(|t| t.playhead.0)
                .unwrap_or(0);
            store.dispatch_playback(snapshort_usecases::PlaybackCommand::Seek {
                frame: snapshort_domain::Frame((cur + delta).max(0)),
            });
        },
        move || Text(label),
    )
    .modifier(Modifier::new().height(32.0))
}

fn audio_channel(name: &str, level: f32) -> View {
    let th = theme();
    let bar_height = 100.0 * level;

    Column(
        Modifier::new()
            .width(52.0)
            .align_items(repose_core::AlignItems::Center),
    )
    .child((
        Box(Modifier::new()
            .width(22.0)
            .height(100.0)
            .background(th.surface)
            .border(1.0, th.outline, 999.0)
            .clip_rounded(999.0))
        .child(
            Column(
                Modifier::new()
                    .fill_max_size()
                    .justify_content(repose_core::JustifyContent::End),
            )
            .child((Box(Modifier::new()
                .fill_max_width()
                .height(bar_height)
                .background(th.primary)),)),
        ),
        v_spacer(6.0),
        Text(name).size(10.0).color(th.on_surface_variant),
    ))
}
