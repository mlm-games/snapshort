use super::dnd::{as_drag_payload, AssetDragPayload};
use crate::state::Store;
use miniter_domain::{Clip, ClipId, ClipKind, MediaDuration, Timestamp};
use miniter_usecases::EditCommand;
use repose_core::{view::View, Color, Modifier};
use repose_core::prelude::theme;
use repose_material::material3;
use repose_material::Icon;
use repose_ui::scroll::{remember_scroll_state, ScrollArea};
use repose_ui::{Box, Button, Column, Row, Spacer, Text, TextStyle, ViewExt};
use miniter_domain::{TrackId, TrackKind};
use snapshort_ui_core::Icons;
use snapshort_usecases::{Asset, AssetCommand, AssetId, AssetType};
use std::rc::Rc;

pub fn assets_panel(store: Rc<Store>) -> View {
    let th = theme();
    let assets = store.state.assets.get();

    let header = Row(
        Modifier::new()
            .fill_max_width()
            .height(40.0)
            .background(th.surface)
            .border(1.0, th.outline, 0.0)
            .padding_values(repose_core::PaddingValues {
                left: 12.0,
                right: 12.0,
                top: 8.0,
                bottom: 8.0,
            })
            .align_items(repose_core::AlignItems::Center),
    )
    .child(vec![
        Icon(Icons::movie).size(18.0).color(th.primary),
        Box(Modifier::new().width(8.0)),
        Text("Assets").size(13.0).color(th.on_surface),
        Box(Modifier::new().flex_grow(1.0)),
        Text(format!("{} items", assets.len()))
            .size(11.0)
            .color(th.on_surface_variant),
    ]);

    let list = if assets.is_empty() {
        Column(
            Modifier::new()
                .fill_max_size()
                .align_items(repose_core::AlignItems::Center)
                .justify_content(repose_core::JustifyContent::Center)
                .padding(16.0),
        )
        .child((
            Text("No assets yet").size(13.0).color(th.on_surface_variant),
            Box(Modifier::new().height(6.0)),
            Text("Import media to get started.")
                .size(11.0)
                .color(th.on_surface_variant.with_alpha(160)),
        ))
    } else {
        let rows: Vec<View> = assets
            .iter()
            .enumerate()
            .map(|(idx, asset)| asset_item(asset, idx, store.clone()))
            .collect();

        ScrollArea(
            Modifier::new().fill_max_size(),
            remember_scroll_state("assets_list"),
            Column(Modifier::new().fill_max_width()).child(rows),
        )
    };

    let footer = Row(
        Modifier::new()
            .fill_max_width()
            .height(56.0)
            .background(th.surface)
            .border(1.0, th.outline, 0.0)
            .padding_values(repose_core::PaddingValues {
                left: 12.0,
                right: 12.0,
                top: 10.0,
                bottom: 10.0,
            })
            .align_items(repose_core::AlignItems::Center),
    )
    .child((
        material3::FilledButton(
            Modifier::new().width(180.0),
            {
                let store = store.clone();
                move || {
                    if let Some(paths) = rfd::FileDialog::new().pick_files() {
                        store.dispatch_asset(AssetCommand::Import { paths });
                    }
                }
            },
            move || Text("Import Media"),
        ),
        Spacer().modifier(Modifier::new().flex_grow(1.0)),
        Text("Tip: drag assets into the timeline")
            .size(11.0)
            .color(th.on_surface_variant),
    ));

    Column(Modifier::new().fill_max_size().background(th.background)).child((
        header,
        Box(Modifier::new().height(1.0).background(th.outline.with_alpha(128))),
        Row(Modifier::new().flex_grow(1.0)).child(list),
        Box(Modifier::new().height(1.0).background(th.outline.with_alpha(128))),
        footer,
    ))
}

fn asset_item(asset: &Asset, idx: usize, store: Rc<Store>) -> View {
    let th = theme();

    let (type_icon, type_label, type_tint) = match asset.asset_type {
        AssetType::Video => (Icons::movie, "Video", th.primary),
        AssetType::Audio => (Icons::music_note, "Audio", th.tertiary),
        AssetType::Image => (Icons::image, "Image", th.secondary),
        AssetType::Sequence => (Icons::burst_mode, "Sequence", th.secondary),
    };

    let status_label = match &asset.status {
        snapshort_usecases::AssetStatus::Pending => "Pending".to_string(),
        snapshort_usecases::AssetStatus::Analyzing => "Analyzing".to_string(),
        snapshort_usecases::AssetStatus::Ready => "Ready".to_string(),
        snapshort_usecases::AssetStatus::ProxyGenerating { progress } => {
            format!("Proxy {progress}%")
        }
        snapshort_usecases::AssetStatus::ProxyReady => "Proxy Ready".to_string(),
        snapshort_usecases::AssetStatus::Offline => "Offline".to_string(),
        snapshort_usecases::AssetStatus::Error(e) => format!("Error: {e}"),
    };

    let duration = asset
        .media_info
        .as_ref()
        .map(|m| format!("{:.1}s", (m.duration_ms as f64) / 1000.0))
        .unwrap_or_else(|| "-".to_string());

    let selected = store.state.selected_asset_id.get() == Some(asset.id);

    let bg = if selected {
        th.primary_container.with_alpha(80)
    } else {
        th.background
    };

    let border = if selected {
        th.primary
    } else {
        th.outline.with_alpha(160)
    };

    let row = Row(
        Modifier::new()
            .key(idx as u64)
            .fill_max_width()
            .height(56.0)
            .padding_values(repose_core::PaddingValues {
                left: 12.0,
                right: 12.0,
                top: 8.0,
                bottom: 8.0,
            })
            .align_items(repose_core::AlignItems::Center)
            .background(bg)
            .border(1.0, border, 10.0)
            .clip_rounded(10.0)
            .on_drag_start({
                let asset_id = asset.id;
                move |_| Some(as_drag_payload(AssetDragPayload { asset_id }))
            }),
    )
    .child(vec![
        Box(
            Modifier::new()
                .size(40.0, 40.0)
                .background(th.surface_variant)
                .clip_rounded(10.0)
                .align_items(repose_core::AlignItems::Center)
                .justify_content(repose_core::JustifyContent::Center),
        )
        .child(Icon(type_icon).size(20.0).color(type_tint)),
        Box(Modifier::new().width(10.0)),
        Column(Modifier::new().flex_grow(1.0)).child((
            Text(asset.name.clone())
                .size(12.0)
                .color(th.on_surface)
                .single_line()
                .overflow_ellipsize(),
            Box(Modifier::new().height(4.0)),
            Row(Modifier::new().align_items(repose_core::AlignItems::Center).gap(8.0)).child((
                chip(type_label, type_tint, type_tint.with_alpha(24)),
                chip(&duration, th.on_surface_variant, th.surface_variant),
                chip(&status_label, th.on_surface_variant, th.surface_variant),
            )),
        )),
        Row(Modifier::new().align_items(repose_core::AlignItems::Center).gap(4.0)).child((
            {
                let is_ready = asset.media_info.is_some();
                let add_btn = if is_ready {
                    material3::IconButton(
                        Icon(Icons::add).size(16.0),
                        {
                            let store = store.clone();
                            let asset_id = asset.id;
                            move || {
                                let assets = store.state.assets.get();
                                let Some(asset) = assets.iter().find(|a| a.id == asset_id) else { return };
                                let Some(ref info) = asset.media_info else {
                                    store.state.status_msg.set("Asset is still being analyzed".into());
                                    return;
                                };
                                let Some(tl) = store.state.timeline.get() else { return };
                                let track_id = tl.tracks.first().map(|t| t.id).unwrap_or(TrackId::new());
                                let duration_us = (info.duration_ms as i64 * 1000).max(1);
                                let is_video = matches!(
                                    asset.asset_type,
                                    AssetType::Video | AssetType::Image | AssetType::Sequence
                                );
                                let (width, height, fps) = info.primary_video()
                                    .map(|v| (v.width, v.height, v.fps))
                                    .unwrap_or((1920, 1080, 30.0));
                                let (sample_rate, channels) = info.primary_audio()
                                    .map(|a| (a.sample_rate, a.channels))
                                    .unwrap_or((48000, 2));
                                let clip_kind = if is_video {
                                    ClipKind::Video(miniter_domain::VideoClip {
                                        source_path: asset.effective_path().to_string_lossy().to_string(),
                                        width,
                                        height,
                                        fps,
                                        filters: vec![],
                                        audio_filters: vec![],
                                    })
                                } else {
                                    ClipKind::Audio(miniter_domain::AudioClip {
                                        source_path: asset.effective_path().to_string_lossy().to_string(),
                                        sample_rate,
                                        channels,
                                        filters: vec![],
                                    })
                                };
                                let clip = Clip {
                                    id: ClipId::new(),
                                    timeline_start: Timestamp(0),
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
                                store.dispatch_edit(EditCommand::AddClip { track_id, clip });
                            }
                        },
                    )
                } else {
                    material3::IconButton(
                        Icon(Icons::info).size(16.0),
                        {
                            let store = store.clone();
                            move || {
                                store.state.status_msg.set("Asset is still being analyzed".into());
                            }
                        },
                    )
                };
                add_btn
            },
            material3::IconButton(
                Icon(Icons::bolt).size(16.0),
                {
                    let store = store.clone();
                    let asset_id = asset.id;
                    move || {
                        store.dispatch_asset(AssetCommand::GenerateProxy { asset_id });
                    }
                },
            ),
            material3::IconButton(
                Icon(Icons::delete).size(16.0),
                {
                    let store = store.clone();
                    let asset_id = asset.id;
                    move || {
                        store.dispatch_asset(AssetCommand::Delete { asset_id });
                    }
                },
            ),
        )),
    ]);

    Button(row, {
        let store = store.clone();
        let asset_id = asset.id;
        move || {
            store.state.selected_asset_id.set(Some(asset_id));
            store.state.selected_clip_id.set(None);
        }
    })
}

fn chip(label: &str, fg: Color, bg: Color) -> View {
    Box(
        Modifier::new()
            .padding_values(repose_core::PaddingValues {
                left: 8.0,
                right: 8.0,
                top: 3.0,
                bottom: 3.0,
            })
            .background(bg)
            .clip_rounded(999.0),
    )
    .child(Text(label).size(10.0).color(fg))
}
