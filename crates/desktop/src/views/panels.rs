//! Panel definitions for the docking system

use crate::state::Store;
use repose_core::prelude::theme;
use repose_core::{Color, Modifier, View};
use repose_docking::{DockKind, DockNode, DockPanel, DockState, PanelId, SplitDir};
use repose_material::material3;
use repose_material::Icon;
use repose_ui::scroll::{remember_scroll_state, ScrollArea};
use repose_ui::{Box, Column, Image, ImageExt, Row, Text, TextStyle, ViewExt};
use snapshort_infra_render::{OutputFormat, QualityPreset};
use snapshort_ui_core::{Icons, colors};
use snapshort_usecases::{PlaybackCommand, PreviewCommand, RenderCommand};
use std::rc::Rc;

use super::inspector;

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
            ratio: 0.45,
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
    use miniter_domain::Timestamp;

    let th = theme();

    let playhead_us = store.state.playhead.get().0;

    let store_for_undo = store.clone();
    let store_for_redo = store.clone();
    let last_render_plan = store.state.last_render_plan_summary.get();
    let preview_handle = store.state.preview_image_handle.get();
    let playback_state = store.state.playback_state.get();
    store.dispatch_preview(PreviewCommand::RequestFrame {
        timestamp: Timestamp(playhead_us),
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
        .align_items(repose_core::AlignItems::CENTER))
    .child(vec![
        material3::IconButton(Icon(Icons::undo).size(18.0), {
            let store = store_for_undo.clone();
            move || store.dispatch_undo()
        }, Default::default()),
        h_spacer(6.0),
        material3::IconButton(Icon(Icons::redo).size(18.0), {
            let store = store_for_redo.clone();
            move || store.dispatch_redo()
        }, Default::default()),
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
        Box(Modifier::new()
            .width(1.0)
            .height(16.0)
            .background(th.outline.with_alpha(128))),
        h_spacer(14.0),
        Text(format!("Time: {}", format_us(playhead_us)))
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
                .align_items(repose_core::AlignItems::CENTER)
                .justify_content(repose_core::AlignContent::CENTER))
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
                .align_items(repose_core::AlignItems::CENTER))
            .child((
                Text(format!("{} ({})", format_us(playhead_us), playback_state))
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
        .justify_content(repose_core::AlignContent::CENTER)
        .align_items(repose_core::AlignItems::CENTER))
    .child(vec![
        playback_button(
            store.clone(),
            Icons::skip_previous,
            PlaybackCommand::Seek { timestamp: Timestamp(0) },
        ),
        h_spacer(12.0),
        playback_seek_rel(store.clone(), Icons::fast_rewind, -24),
        h_spacer(12.0),
        playback_button(store.clone(), Icons::play_arrow, PlaybackCommand::Play),
        h_spacer(12.0),
        playback_button(store.clone(), Icons::pause, PlaybackCommand::Pause),
        h_spacer(12.0),
        playback_button(store.clone(), Icons::stop, PlaybackCommand::Stop),
        h_spacer(12.0),
        playback_seek_rel(store.clone(), Icons::fast_forward, 24),
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
    inspector::inspector_panel(store)
}

fn history_content() -> View {
    let th = theme();
    ScrollArea(
        Modifier::new().fill_max_size(),
        remember_scroll_state("history"),
        Column(Modifier::new().fill_max_width().padding(10.0)).child((
            Text("History").size(12.0).color(th.on_surface_variant),
            v_spacer(8.0),
            Text("Project Created").size(11.0).color(th.on_surface),
            Text("Timeline Created").size(11.0).color(th.on_surface),
        )),
    )
}

fn media_browser_content() -> View {
    let th = theme();
    ScrollArea(
        Modifier::new().fill_max_size(),
        remember_scroll_state("media_browser"),
        Column(Modifier::new().fill_max_width().padding(10.0)).child((
            Text("Media Browser")
                .size(12.0)
                .color(th.on_surface_variant),
            v_spacer(8.0),
            Text("Browse your media files here.")
                .size(11.0)
                .color(th.on_surface_variant.with_alpha(160)),
        )),
    )
}

struct EffectEntry {
    name: &'static str,
    icon: repose_material::Symbol,
}

struct EffectCategory {
    name: &'static str,
    icon: repose_material::Symbol,
    effects: &'static [EffectEntry],
}

static EFFECTS: &[EffectCategory] = &[
    EffectCategory {
        name: "Color Correction",
        icon: Icons::tune,
        effects: &[
            EffectEntry { name: "Brightness / Contrast", icon: Icons::tune },
            EffectEntry { name: "Color Balance", icon: Icons::tune },
            EffectEntry { name: "Hue / Saturation", icon: Icons::palette },
            EffectEntry { name: "Levels", icon: Icons::tune },
            EffectEntry { name: "Curves", icon: Icons::tune },
            EffectEntry { name: "LUT", icon: Icons::auto_fix },
        ],
    },
    EffectCategory {
        name: "Blur & Sharpen",
        icon: Icons::blur_on,
        effects: &[
            EffectEntry { name: "Gaussian Blur", icon: Icons::blur_on },
            EffectEntry { name: "Box Blur", icon: Icons::blur_on },
            EffectEntry { name: "Sharpen", icon: Icons::blur_on },
            EffectEntry { name: "Unsharp Mask", icon: Icons::blur_on },
        ],
    },
    EffectCategory {
        name: "Transform",
        icon: Icons::transform,
        effects: &[
            EffectEntry { name: "Position & Scale", icon: Icons::transform },
            EffectEntry { name: "Rotate", icon: Icons::straighten },
            EffectEntry { name: "Crop", icon: Icons::crop },
            EffectEntry { name: "Flip (Horizontal)", icon: Icons::transform },
            EffectEntry { name: "Flip (Vertical)", icon: Icons::transform },
        ],
    },
    EffectCategory {
        name: "Keying",
        icon: Icons::layers,
        effects: &[
            EffectEntry { name: "Chroma Key (Green Screen)", icon: Icons::layers },
            EffectEntry { name: "Luma Key", icon: Icons::layers },
            EffectEntry { name: "Spill Suppression", icon: Icons::layers },
        ],
    },
    EffectCategory {
        name: "Stylize",
        icon: Icons::auto_fix,
        effects: &[
            EffectEntry { name: "Glow", icon: Icons::flash_on },
            EffectEntry { name: "Sepia", icon: Icons::palette },
            EffectEntry { name: "Pixelate", icon: Icons::filter },
            EffectEntry { name: "Vignette", icon: Icons::filter },
        ],
    },
    EffectCategory {
        name: "Audio EQ & Filters",
        icon: Icons::equalizer,
        effects: &[
            EffectEntry { name: "Parametric EQ", icon: Icons::equalizer },
            EffectEntry { name: "High Pass Filter", icon: Icons::equalizer },
            EffectEntry { name: "Low Pass Filter", icon: Icons::equalizer },
            EffectEntry { name: "Band Pass Filter", icon: Icons::equalizer },
        ],
    },
    EffectCategory {
        name: "Audio Dynamics",
        icon: Icons::volume_up,
        effects: &[
            EffectEntry { name: "Compressor", icon: Icons::volume_up },
            EffectEntry { name: "Limiter", icon: Icons::volume_up },
            EffectEntry { name: "Noise Gate", icon: Icons::volume_up },
            EffectEntry { name: "Normalizer", icon: Icons::volume_up },
        ],
    },
    EffectCategory {
        name: "Audio Time",
        icon: Icons::music_video,
        effects: &[
            EffectEntry { name: "Reverb", icon: Icons::music_video },
            EffectEntry { name: "Delay / Echo", icon: Icons::music_video },
            EffectEntry { name: "Pitch Shift", icon: Icons::music_video },
            EffectEntry { name: "Speed Change", icon: Icons::music_video },
        ],
    },
];

fn effect_row(effect: &EffectEntry, th: &repose_core::Theme) -> View {
    Row(Modifier::new()
        .fill_max_width()
        .height(28.0)
        .padding_values(repose_core::PaddingValues { left: 12.0, right: 8.0, top: 2.0, bottom: 2.0 })
        .align_items(repose_core::AlignItems::CENTER)
        .cursor(repose_core::CursorIcon::Pointer))
    .child(vec![
        Icon(effect.icon).size(14.0).color(th.on_surface_variant),
        h_spacer(8.0),
        Text(effect.name).size(11.0).color(th.on_surface),
    ])
}

fn category_section(category: &EffectCategory, th: &repose_core::Theme) -> View {
    let mut children: Vec<View> = Vec::new();
    children.push(
        Row(Modifier::new()
            .fill_max_width()
            .height(30.0)
            .padding_values(repose_core::PaddingValues { left: 8.0, right: 8.0, top: 4.0, bottom: 2.0 })
            .align_items(repose_core::AlignItems::CENTER))
        .child(vec![
            Icon(category.icon).size(16.0).color(th.primary),
            h_spacer(6.0),
            Text(category.name).size(12.0).color(th.primary),
        ]),
    );
    for effect in category.effects {
        children.push(effect_row(effect, th));
    }
    children.push(v_spacer(4.0));
    Column(Modifier::new().fill_max_width()).child(children)
}

fn effects_content() -> View {
    let th = theme();
    let mut children: Vec<View> = Vec::new();
    children.push(
        Text("Effects")
            .size(13.0)
            .color(th.on_surface_variant),
    );
    children.push(v_spacer(6.0));
    for cat in EFFECTS {
        children.push(category_section(cat, &th));
    }
    ScrollArea(
        Modifier::new().fill_max_size(),
        remember_scroll_state("effects"),
        Column(Modifier::new().fill_max_width().padding(10.0)).child(children),
    )
}

fn audio_mixer_content(store: Rc<Store>) -> View {
    let th = theme();
    let timeline = store.state.timeline.get();
    let audio_tracks: Vec<_> = timeline
        .as_ref()
        .map(|tl| {
            tl.tracks
                .iter()
                .filter(|t| t.kind == miniter_domain::TrackKind::Audio)
                .collect()
        })
        .unwrap_or_default();

    let mut rows: Vec<View> = Vec::new();

    for (i, track) in audio_tracks.iter().enumerate() {
        let label = format!("A{}", i + 1);
        let track_id = track.id;
        let is_muted = track.muted;

        let volumes = store.state.track_volumes.get();
        let vol = volumes.get(&track_id).copied().unwrap_or(1.0);
        let solos = store.state.track_solos.get();
        let is_solo = solos.contains(&track_id);

        let vol_pct = format!("{}%", (vol * 100.0).round() as i32);
        let mute_fg = if is_muted { colors::TEXT_ACCENT } else { colors::TEXT_MUTED };
        let solo_fg = if is_solo { colors::WARNING } else { colors::TEXT_MUTED };

        rows.push(Row(Modifier::new()
            .fill_max_width()
            .padding_values(repose_core::PaddingValues { left: 6.0, right: 6.0, top: 4.0, bottom: 4.0 })
            .align_items(repose_core::AlignItems::CENTER)
        )
        .child((
            Box(Modifier::new().width(28.0)).child(Text(&label).size(11.0).color(th.on_surface).single_line()),
            material3::Slider(vol, (0.0, 2.0), None, {
                let store = store.clone();
                move |value| {
                    let mut vols = store.state.track_volumes.get();
                    vols.insert(track_id, value);
                    store.state.track_volumes.set(vols);
                }
            }, Default::default())
            .modifier(Modifier::new().flex_grow(1.0).height(18.0)),
            h_spacer(4.0),
            Box(Modifier::new().width(36.0)).child(Text(&vol_pct).size(9.0).color(th.on_surface_variant)),
            h_spacer(2.0),
            Box(Modifier::new()
                .size(22.0, 22.0)
                .clip_rounded(4.0)
                .background(if is_muted { colors::ACCENT } else { th.surface_container })
                .align_items(repose_core::AlignItems::CENTER)
                .justify_content(repose_core::AlignContent::CENTER)
                .clickable()
                .on_pointer_down({
                    let store = store.clone();
                    move |_| {
                        use miniter_usecases::EditCommand;
                        store.dispatch_edit(EditCommand::SetTrackMuted {
                            track_id,
                            muted: !store.state.timeline.get()
                                .and_then(|tl| tl.tracks.iter().find(|t| t.id == track_id).cloned())
                                .map(|t| t.muted)
                                .unwrap_or(false),
                        });
                    }
                }))
            .child(Text("M").size(9.0).color(mute_fg)),
            h_spacer(2.0),
            Box(Modifier::new()
                .size(22.0, 22.0)
                .clip_rounded(4.0)
                .background(if is_solo { colors::WARNING } else { th.surface_container })
                .align_items(repose_core::AlignItems::CENTER)
                .justify_content(repose_core::AlignContent::CENTER)
                .clickable()
                .on_pointer_down({
                    let store = store.clone();
                    move |_| {
                        let mut solos = store.state.track_solos.get();
                        if solos.contains(&track_id) {
                            solos.remove(&track_id);
                        } else {
                            solos.insert(track_id);
                        }
                        store.state.track_solos.set(solos);
                    }
                }))
            .child(Text("S").size(9.0).color(solo_fg)),
        )));
    }

    let master_vol = store.state.master_volume.get();
    rows.push(Row(Modifier::new()
        .fill_max_width()
        .padding_values(repose_core::PaddingValues { left: 6.0, right: 6.0, top: 4.0, bottom: 4.0 })
        .align_items(repose_core::AlignItems::CENTER)
    )
    .child((
        Box(Modifier::new().width(48.0)).child(Text("Master").size(11.0).color(th.on_surface).single_line()),
        material3::Slider(master_vol, (0.0, 2.0), None, {
            let store = store.clone();
            move |value| store.state.master_volume.set(value)
        }, Default::default())
        .modifier(Modifier::new().flex_grow(1.0).height(18.0)),
        h_spacer(4.0),
        Box(Modifier::new().width(36.0)).child(
            Text(format!("{}%", (master_vol * 100.0).round() as i32))
                .size(9.0).color(th.on_surface_variant)),
    )));

    ScrollArea(
        Modifier::new().fill_max_size(),
        remember_scroll_state("audio_mixer"),
        Column(Modifier::new().fill_max_width().padding(8.0)).child((
            Text("Audio Mixer").size(12.0).color(th.on_surface_variant),
            v_spacer(4.0),
            Column(Modifier::new().fill_max_width()).child(rows),
        )),
    )
}

fn export_panel_content(store: Rc<Store>) -> View {
    let th = theme();

    let export_path = store.state.export_output_path.get();
    let quality = store.state.export_quality.get();
    let last_result = store.state.last_render_result.get();
    let timeline = store.state.timeline.get();
    let clip_count: usize = timeline
        .as_ref()
        .map(|t| t.tracks.iter().map(|tr| tr.clips.len()).sum())
        .unwrap_or(0);

    let quality_idx = quality as i32;
    let quality_labels = ["Draft", "Preview", "Standard", "High", "Master"];

    let export_button = material3::Button(
        Modifier::new().width(160.0),
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
                    track_volumes: store.state.track_volumes.get(),
                    master_volume: store.state.master_volume.get(),
                });
            }
        },
        Default::default(),
        || Text("Export"),
    );

    ScrollArea(
        Modifier::new().fill_max_size(),
        remember_scroll_state("export"),
        Column(Modifier::new().fill_max_width().background(th.background)).child(vec![
            Box(Modifier::new().padding(12.0)).child(
                Column(Modifier::new().fill_max_width()).child((
                    Text("Export").size(14.0).color(th.on_surface),
                    v_spacer(4.0),
                    Text(format!("Timeline clips: {}", clip_count))
                        .size(11.0)
                        .color(th.on_surface_variant),
                )),
            ),
            Box(Modifier::new().height(1.0).background(th.outline.with_alpha(128))),
            Box(Modifier::new().height(12.0)),
            Row(Modifier::new().fill_max_width().padding_values(repose_core::PaddingValues { left: 12.0, right: 12.0, top: 0.0, bottom: 0.0 }).align_items(repose_core::AlignItems::CENTER)).child(vec![
                Text("Output").size(12.0).color(th.on_surface_variant),
                Box(Modifier::new().width(10.0)),
                Text(export_path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "Not set".into()))
                    .size(12.0).color(th.on_surface).single_line(),
                Box(Modifier::new().flex_grow(1.0)),
                material3::TextButton(Modifier::new(), {
                    let store = store.clone();
                    move || {
                        if let Some(path) = rfd::FileDialog::new().set_file_name("export.mp4").save_file() {
                            store.state.export_output_path.set(Some(path));
                        }
                    }
                }, Default::default(), || Text("Choose…")),
            ]),
            Box(Modifier::new().height(16.0)),
            Box(Modifier::new().padding_values(repose_core::PaddingValues { left: 12.0, right: 12.0, top: 0.0, bottom: 0.0 })).child(
                Column(Modifier::new().fill_max_width()).child((
                    Row(Modifier::new().fill_max_width().align_items(repose_core::AlignItems::CENTER)).child((
                        Text("Quality").size(11.0).color(th.on_surface_variant),
                        Box(Modifier::new().flex_grow(1.0)),
                        Text(quality_labels[quality_idx as usize]).size(11.0).color(th.on_surface),
                    )),
                    v_spacer(4.0),
                    material3::Slider(quality_idx as f32, (0.0, 4.0), Some(1.0), {
                        let store = store.clone();
                        move |v| {
                            let preset = match v.round() as i32 {
                                0 => QualityPreset::Draft,
                                1 => QualityPreset::Preview,
                                2 => QualityPreset::Standard,
                                3 => QualityPreset::High,
                                _ => QualityPreset::Master,
                            };
                            store.state.export_quality.set(preset);
                        }
                    }, Default::default()).modifier(Modifier::new().height(28.0).fill_max_width()),
                )),
            ),
            Box(Modifier::new().height(16.0)),
            Box(Modifier::new().padding_values(repose_core::PaddingValues { left: 12.0, right: 12.0, top: 0.0, bottom: 0.0 })).child(
                Row(Modifier::new().fill_max_width().align_items(repose_core::AlignItems::CENTER)).child((
                    export_button,
                    Box(Modifier::new().flex_grow(1.0)),
                )),
            ),
            Box(Modifier::new().height(10.0)),
            Box(Modifier::new().padding_values(repose_core::PaddingValues { left: 12.0, right: 12.0, top: 0.0, bottom: 0.0 })).child(
                kv("Status", last_result.unwrap_or_else(|| "Idle".into())),
            ),
        ]),
    )
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
        .align_items(repose_core::AlignItems::CENTER))
    .child(vec![
        Text(label.into()).size(11.0).color(th.on_surface_variant),
        Box(Modifier::new().flex_grow(1.0)),
        Text(value.into()).size(11.0).color(th.on_surface),
    ])
}

fn playback_button(
    store: Rc<Store>,
    icon: repose_material::Symbol,
    cmd: snapshort_usecases::PlaybackCommand,
) -> View {
    material3::FilledTonalButton(
        Modifier::new().height(32.0),
        move || store.dispatch_playback(cmd.clone()),
        Default::default(),
        move || Icon(icon).size(18.0),
    )
}

fn playback_seek_rel(store: Rc<Store>, icon: repose_material::Symbol, delta_us: i64) -> View {
    use miniter_domain::Timestamp;
    material3::FilledTonalButton(
        Modifier::new().height(32.0),
        move || {
            let cur = store.state.playhead.get().0;
            store.dispatch_playback(PlaybackCommand::Seek {
                timestamp: Timestamp((cur + delta_us).max(0)),
            });
        },
        Default::default(),
        move || Icon(icon).size(18.0),
    )
}

fn format_us(us: i64) -> String {
    let abs = us.unsigned_abs();
    let millis = abs / 1000;
    let secs = millis / 1000;
    let mins = secs / 60;
    let hours = mins / 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}.{:03}", hours, mins % 60, secs % 60, millis % 1000)
    } else if mins > 0 {
        format!("{:02}:{:02}.{:03}", mins, secs % 60, millis % 1000)
    } else {
        format!("{}.{:03}s", secs, millis % 1000)
    }
}
