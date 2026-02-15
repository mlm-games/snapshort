//! Panel definitions for the docking system
//!
//! This module defines all dockable panels and their content factories.

use crate::state::Store;
use repose_core::request_frame;
use repose_core::{Color, Modifier, View};
use repose_docking::{DockKind, DockNode, DockPanel, DockState, PanelId, SplitDir};
use repose_ui::{Box, Column, Image, ImageExt, Row, Slider, Text, TextStyle, ViewExt};
use snapshort_infra_render::{OutputFormat, QualityPreset};
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
            content: Rc::new(|| audio_mixer_content()),
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

    // Right panel: Inspector, History, Audio Mixer, Export as tabs
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

    // Program + Source in shared tabs
    let program_monitor = DockNode {
        id: 12,
        kind: DockKind::Tabs {
            tabs: vec![PANEL_PROGRAM_MONITOR, PANEL_SOURCE_MONITOR],
            active: Some(PANEL_PROGRAM_MONITOR),
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

    // Center area: monitors on top, timeline on bottom
    let center_split = DockNode {
        id: 16,
        kind: DockKind::Split {
            dir: SplitDir::Vertical,
            ratio: 0.58,
            a: std::boxed::Box::new(program_monitor),
            b: std::boxed::Box::new(timeline),
        },
    };

    // Center + right panel split
    let center_right_split = DockNode {
        id: 17,
        kind: DockKind::Split {
            dir: SplitDir::Horizontal,
            ratio: 0.78,
            a: std::boxed::Box::new(center_split),
            b: std::boxed::Box::new(right_tabs),
        },
    };

    // Root: left + (center + right)
    let root = DockNode {
        id: 1,
        kind: DockKind::Split {
            dir: SplitDir::Horizontal,
            ratio: 0.2,
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
    let last_render_plan = store.state.last_render_plan_summary.get();
    let preview_handle = store.state.preview_image_handle.get();
    let preview_generation = store
        .preview_generation
        .load(std::sync::atomic::Ordering::Relaxed);
    let playback_state = store.state.playback_state.get();
    let store_for_preview = store.clone();
    ensure_preview_frame(store.clone(), playhead, preview_generation);

    // Toolbar
    let toolbar = Row(Modifier::new()
        .fill_max_width()
        .height(36.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding_values(repose_core::PaddingValues {
            left: 10.0,
            right: 10.0,
            top: 6.0,
            bottom: 6.0,
        })
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        icon_button("↶", {
            let store = store_for_undo.clone();
            move || store.dispatch_timeline(TimelineCommand::Undo)
        })
        .modifier(Modifier::new().padding(4.0)),
        h_spacer(6.0),
        icon_button("↷", {
            let store = store_for_redo.clone();
            move || store.dispatch_timeline(TimelineCommand::Redo)
        })
        .modifier(Modifier::new().padding(4.0)),
        h_spacer(10.0),
        Box(Modifier::new()
            .width(1.0)
            .height(16.0)
            .background(colors::BORDER)),
        h_spacer(10.0),
        Text("100%").size(11.0).color(colors::TEXT_PRIMARY),
        Box(Modifier::new().flex_grow(1.0)),
        Text("Full").size(11.0).color(colors::TEXT_MUTED),
        h_spacer(14.0),
        Box(Modifier::new()
            .width(1.0)
            .height(16.0)
            .background(colors::BORDER)),
        h_spacer(14.0),
        Text(format!("Frame: {playhead}"))
            .size(11.0)
            .color(colors::TEXT_PRIMARY),
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
                .background(colors::BG_DARK)
                .border(1.0, colors::BORDER, 0.0)
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
            )
            .modifier(Modifier::new().on_pointer_down({
                move |_| {
                    request_frame();
                    store_for_preview
                        .preview_generation
                        .store(preview_generation + 1, std::sync::atomic::Ordering::Relaxed);
                }
            })),
            v_spacer(6.0),
            Row(Modifier::new()
                .fill_max_width()
                .align_items(repose_core::AlignItems::Center))
            .child((
                Text(format!("Frame: {} ({})", playhead, playback_state))
                    .size(10.0)
                    .color(colors::TEXT_MUTED),
                Box(Modifier::new().flex_grow(1.0)),
                Text(last_render_plan.unwrap_or_else(|| "Render plan not generated".into()))
                    .size(10.0)
                    .color(colors::TEXT_DISABLED),
            )),
        )),
    );

    // Playback controls
    let controls = Row(Modifier::new()
        .fill_max_width()
        .height(44.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding_values(repose_core::PaddingValues {
            left: 12.0,
            right: 12.0,
            top: 6.0,
            bottom: 6.0,
        })
        .justify_content(repose_core::JustifyContent::Center))
    .child(vec![
        playback_button(
            store.clone(),
            "⏮",
            PlaybackCommand::Seek { frame: Frame(0) },
        ),
        h_spacer(20.0),
        playback_seek_rel(store.clone(), "◀", -24),
        h_spacer(20.0),
        playback_button(store.clone(), "▶", PlaybackCommand::Play),
        h_spacer(20.0),
        playback_button(store.clone(), "⏸", PlaybackCommand::Pause),
        h_spacer(20.0),
        playback_button(store.clone(), "⏹", PlaybackCommand::Stop),
        h_spacer(20.0),
        playback_seek_rel(store.clone(), "⏭", 24),
    ]);

    Column(Modifier::new().fill_max_size().background(colors::BG_DARK))
        .child((toolbar, preview, controls))
}

fn ensure_preview_frame(store: Rc<Store>, playhead: i64, generation: u64) {
    use std::sync::atomic::Ordering;

    let timeline = store.state.timeline.get();
    let Some(tl) = timeline else {
        return;
    };

    let clip = tl
        .clips
        .iter()
        .find(|c| c.enabled && c.clip_type == snapshort_domain::ClipType::Video);
    let Some(clip) = clip else {
        return;
    };
    let Some(asset_id) = clip.asset_id else {
        return;
    };

    let key = (asset_id, playhead);
    if store.preview_last_key.borrow().as_ref() == Some(&key)
        && store.preview_generation.load(Ordering::Relaxed) == generation
    {
        return;
    }

    if store
        .preview_in_flight
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    *store.preview_last_key.borrow_mut() = Some(key);

    let assets = store.state.assets.get();
    let asset = assets.iter().find(|a| a.id == asset_id).cloned();
    let Some(asset) = asset else {
        store.preview_in_flight.store(false, Ordering::SeqCst);
        return;
    };

    let asset_path = asset.effective_path().clone();
    let render_handle = store.state.preview_image_handle.get();
    let fps = tl.settings.fps;
    let resolution = tl.settings.resolution;
    let preview_generation = store.preview_generation.load(Ordering::Relaxed);

    let preview_in_flight = store.preview_in_flight.clone();
    let preview_generation_ptr = store.preview_generation.clone();
    let render_ctx = store.render_ctx.borrow().clone();
    let Some(render_ctx) = render_ctx else {
        return;
    };
    std::thread::spawn(move || {
        let output = std::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-ss")
            .arg(format!("{:.3}", playhead as f64 / fps.as_f64()))
            .arg("-i")
            .arg(asset_path)
            .arg("-vframes")
            .arg("1")
            .arg("-vf")
            .arg(format!(
                "scale={}:{}:flags=lanczos:force_original_aspect_ratio=decrease",
                resolution.width.max(1),
                resolution.height.max(1)
            ))
            .arg("-f")
            .arg("image2")
            .arg("-")
            .output();

        let Ok(output) = output else {
            preview_in_flight.store(false, Ordering::SeqCst);
            return;
        };
        if !output.status.success() {
            preview_in_flight.store(false, Ordering::SeqCst);
            return;
        }

        let bytes = output.stdout;
        let rgba = match image::load_from_memory(&bytes) {
            Ok(img) => img.to_rgba8(),
            Err(_) => {
                preview_in_flight.store(false, Ordering::SeqCst);
                return;
            }
        };

        let (w, h) = rgba.dimensions();
        render_ctx.set_image_rgba8(render_handle, w, h, rgba.into_raw(), true);
        preview_generation_ptr.store(preview_generation + 1, Ordering::Relaxed);
        preview_in_flight.store(false, Ordering::SeqCst);
    });
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
            let (pos_x, pos_y) = clip.effects.position;
            let (scale_x, scale_y) = clip.effects.scale;
            let rotation = clip.effects.rotation;
            let brightness = clip.effects.brightness;
            let contrast = clip.effects.contrast;
            let saturation = clip.effects.saturation;
            let store_for_speed = store.clone();
            let store_for_opacity = store.clone();
            let store_for_pos_x = store.clone();
            let store_for_pos_y = store.clone();
            let store_for_scale_x = store.clone();
            let store_for_scale_y = store.clone();
            let store_for_rotation = store.clone();
            let store_for_brightness = store.clone();
            let store_for_contrast = store.clone();
            let store_for_saturation = store.clone();

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

            let effects_section = Column(Modifier::new().fill_max_width()).child(vec![
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
                v_spacer(6.0),
                Text("Transform").size(11.0).color(colors::TEXT_MUTED),
                property_slider(
                    "Position X",
                    pos_x,
                    (-1000.0, 1000.0),
                    format!("{:.0}px", pos_x),
                    move |value| {
                        store_for_pos_x.dispatch_timeline(
                            snapshort_usecases::TimelineCommand::SetClipPosition {
                                clip_id,
                                x: value,
                                y: pos_y,
                            },
                        );
                    },
                ),
                property_slider(
                    "Position Y",
                    pos_y,
                    (-1000.0, 1000.0),
                    format!("{:.0}px", pos_y),
                    move |value| {
                        store_for_pos_y.dispatch_timeline(
                            snapshort_usecases::TimelineCommand::SetClipPosition {
                                clip_id,
                                x: pos_x,
                                y: value,
                            },
                        );
                    },
                ),
                property_slider(
                    "Scale X",
                    scale_x,
                    (0.1, 3.0),
                    format!("{:.2}x", scale_x),
                    move |value| {
                        store_for_scale_x.dispatch_timeline(
                            snapshort_usecases::TimelineCommand::SetClipScale {
                                clip_id,
                                x: value,
                                y: scale_y,
                            },
                        );
                    },
                ),
                property_slider(
                    "Scale Y",
                    scale_y,
                    (0.1, 3.0),
                    format!("{:.2}x", scale_y),
                    move |value| {
                        store_for_scale_y.dispatch_timeline(
                            snapshort_usecases::TimelineCommand::SetClipScale {
                                clip_id,
                                x: scale_x,
                                y: value,
                            },
                        );
                    },
                ),
                property_slider(
                    "Rotation",
                    rotation,
                    (-180.0, 180.0),
                    format!("{:.0}°", rotation),
                    move |value| {
                        store_for_rotation.dispatch_timeline(
                            snapshort_usecases::TimelineCommand::SetClipRotation {
                                clip_id,
                                rotation: value,
                            },
                        );
                    },
                ),
                v_spacer(6.0),
                Text("Color").size(11.0).color(colors::TEXT_MUTED),
                property_slider(
                    "Brightness",
                    brightness,
                    (-1.0, 1.0),
                    format!("{:.0}%", brightness * 100.0),
                    move |value| {
                        store_for_brightness.dispatch_timeline(
                            snapshort_usecases::TimelineCommand::SetClipBrightness {
                                clip_id,
                                brightness: value,
                            },
                        );
                    },
                ),
                property_slider(
                    "Contrast",
                    contrast,
                    (-1.0, 1.0),
                    format!("{:.0}%", contrast * 100.0),
                    move |value| {
                        store_for_contrast.dispatch_timeline(
                            snapshort_usecases::TimelineCommand::SetClipContrast {
                                clip_id,
                                contrast: value,
                            },
                        );
                    },
                ),
                property_slider(
                    "Saturation",
                    saturation,
                    (-1.0, 1.0),
                    format!("{:.0}%", saturation * 100.0),
                    move |value| {
                        store_for_saturation.dispatch_timeline(
                            snapshort_usecases::TimelineCommand::SetClipSaturation {
                                clip_id,
                                saturation: value,
                            },
                        );
                    },
                ),
            ]);

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

fn export_panel_content(store: Rc<Store>) -> View {
    use snapshort_usecases::RenderCommand;

    let export_path = store.state.export_output_path.get();
    let format = store.state.export_format.get();
    let quality = store.state.export_quality.get();
    let use_hw = store.state.export_use_hw_accel.get();
    let last_result = store.state.last_render_result.get();
    let timeline = store.state.timeline.get();
    let clip_count = timeline.as_ref().map(|t| t.clips.len()).unwrap_or(0);

    let format_label = match format {
        OutputFormat::Mp4H264 => "MP4 H.264",
        OutputFormat::Mp4H265 => "MP4 H.265",
        OutputFormat::WebmVp9 => "WebM VP9",
        OutputFormat::MovProRes => "MOV ProRes",
        OutputFormat::PngSequence => "PNG Sequence",
        OutputFormat::JpegSequence => "JPEG Sequence",
    };

    let quality_label = match quality {
        QualityPreset::Draft => "Draft",
        QualityPreset::Preview => "Preview",
        QualityPreset::Standard => "Standard",
        QualityPreset::High => "High",
        QualityPreset::Master => "Master",
    };

    let header = Column(Modifier::new().fill_max_width().padding(8.0)).child((
        Text("Export").size(12.0).color(colors::TEXT_PRIMARY),
        v_spacer(4.0),
        Text(format!("Timeline clips: {}", clip_count))
            .size(10.0)
            .color(colors::TEXT_MUTED),
    ));

    let output_row = Row(Modifier::new()
        .fill_max_width()
        .height(30.0)
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        Text("Output").size(11.0).color(colors::TEXT_MUTED),
        Box(Modifier::new().width(8.0)),
        Text(
            export_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "Not set".into()),
        )
        .size(10.0)
        .color(colors::TEXT_PRIMARY),
        Box(Modifier::new().flex_grow(1.0)),
        snapshort_ui_core::icon_button("📁", {
            let store = store.clone();
            move || {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("export.mp4")
                    .save_file()
                {
                    store.state.export_output_path.set(Some(path));
                }
            }
        })
        .modifier(Modifier::new().width(32.0).height(24.0)),
    ]);

    let format_row = Row(Modifier::new()
        .fill_max_width()
        .height(28.0)
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        Text("Format").size(11.0).color(colors::TEXT_MUTED),
        Box(Modifier::new().width(8.0)),
        Text(format_label).size(10.0).color(colors::TEXT_PRIMARY),
        Box(Modifier::new().flex_grow(1.0)),
        snapshort_ui_core::icon_button("◀", {
            let store = store.clone();
            move || {
                let next = match store.state.export_format.get() {
                    OutputFormat::Mp4H264 => OutputFormat::JpegSequence,
                    OutputFormat::Mp4H265 => OutputFormat::Mp4H264,
                    OutputFormat::WebmVp9 => OutputFormat::Mp4H265,
                    OutputFormat::MovProRes => OutputFormat::WebmVp9,
                    OutputFormat::PngSequence => OutputFormat::MovProRes,
                    OutputFormat::JpegSequence => OutputFormat::PngSequence,
                };
                store.state.export_format.set(next);
            }
        })
        .modifier(Modifier::new().width(32.0).height(24.0)),
        snapshort_ui_core::icon_button("▶", {
            let store = store.clone();
            move || {
                let next = match store.state.export_format.get() {
                    OutputFormat::Mp4H264 => OutputFormat::Mp4H265,
                    OutputFormat::Mp4H265 => OutputFormat::WebmVp9,
                    OutputFormat::WebmVp9 => OutputFormat::MovProRes,
                    OutputFormat::MovProRes => OutputFormat::PngSequence,
                    OutputFormat::PngSequence => OutputFormat::JpegSequence,
                    OutputFormat::JpegSequence => OutputFormat::Mp4H264,
                };
                store.state.export_format.set(next);
            }
        })
        .modifier(Modifier::new().width(32.0).height(24.0)),
    ]);

    let quality_row = Row(Modifier::new()
        .fill_max_width()
        .height(28.0)
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        Text("Quality").size(11.0).color(colors::TEXT_MUTED),
        Box(Modifier::new().width(8.0)),
        Text(quality_label).size(10.0).color(colors::TEXT_PRIMARY),
        Box(Modifier::new().flex_grow(1.0)),
        snapshort_ui_core::icon_button("◀", {
            let store = store.clone();
            move || {
                let next = match store.state.export_quality.get() {
                    QualityPreset::Draft => QualityPreset::Master,
                    QualityPreset::Preview => QualityPreset::Draft,
                    QualityPreset::Standard => QualityPreset::Preview,
                    QualityPreset::High => QualityPreset::Standard,
                    QualityPreset::Master => QualityPreset::High,
                };
                store.state.export_quality.set(next);
            }
        })
        .modifier(Modifier::new().width(32.0).height(24.0)),
        snapshort_ui_core::icon_button("▶", {
            let store = store.clone();
            move || {
                let next = match store.state.export_quality.get() {
                    QualityPreset::Draft => QualityPreset::Preview,
                    QualityPreset::Preview => QualityPreset::Standard,
                    QualityPreset::Standard => QualityPreset::High,
                    QualityPreset::High => QualityPreset::Master,
                    QualityPreset::Master => QualityPreset::Draft,
                };
                store.state.export_quality.set(next);
            }
        })
        .modifier(Modifier::new().width(32.0).height(24.0)),
    ]);

    let hw_row = Row(Modifier::new()
        .fill_max_width()
        .height(28.0)
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        Text("Hardware accel").size(11.0).color(colors::TEXT_MUTED),
        Box(Modifier::new().width(8.0)),
        Text(if use_hw { "On" } else { "Off" })
            .size(10.0)
            .color(colors::TEXT_PRIMARY),
        Box(Modifier::new().flex_grow(1.0)),
        snapshort_ui_core::icon_button(if use_hw { "✅" } else { "⬜" }, {
            let store = store.clone();
            move || {
                store
                    .state
                    .export_use_hw_accel
                    .set(!store.state.export_use_hw_accel.get())
            }
        })
        .modifier(Modifier::new().width(32.0).height(24.0)),
    ]);

    let export_button = snapshort_ui_core::primary_button("Export", {
        let store = store.clone();
        move || {
            let Some(output_path) = store.state.export_output_path.get() else {
                store
                    .state
                    .last_render_result
                    .set(Some("Select an output path".into()));
                return;
            };

            store.dispatch_render(RenderCommand::Export {
                output_path,
                format: store.state.export_format.get(),
                quality: store.state.export_quality.get(),
                use_hardware_accel: store.state.export_use_hw_accel.get(),
            });
        }
    })
    .modifier(Modifier::new().width(160.0));

    let status_row = Column(Modifier::new().fill_max_width()).child((
        Text("Status").size(11.0).color(colors::TEXT_MUTED),
        v_spacer(4.0),
        Text(last_result.unwrap_or_else(|| "Idle".into()))
            .size(10.0)
            .color(colors::TEXT_PRIMARY),
    ));

    Column(Modifier::new().fill_max_size().background(colors::BG_DARK)).child(vec![
        header,
        Box(Modifier::new().height(1.0).background(colors::BORDER)),
        Box(Modifier::new().height(6.0)),
        output_row,
        format_row,
        quality_row,
        hw_row,
        Box(Modifier::new().height(10.0)),
        export_button,
        Box(Modifier::new().height(8.0)),
        status_row,
    ])
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
