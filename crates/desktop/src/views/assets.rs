use super::dnd::{as_drag_payload, AssetDragPayload};
use crate::state::Store;
use miniter_domain::{Clip, ClipId, ClipKind, MediaDuration, Timestamp};
use miniter_usecases::EditCommand;
use repose_core::{view::View, Color, Modifier};
use repose_core::prelude::theme;
use repose_material::material3;
use repose_material::Icon;
use repose_ui::scroll::{remember_scroll_state, ScrollArea};
use repose_ui::textfield::{set_textfield_state, get_textfield_state};
use repose_ui::{BasicTextField, Box, Column, Row, Spacer, Text, TextFieldConfig, TextFieldState, TextStyle, ViewExt};
use repose_core::runtime::remember_state_with_key;
use miniter_domain::TrackId;
use snapshort_ui_core::Icons;
use snapshort_usecases::{Asset, AssetCommand, AssetType};
use std::rc::Rc;

pub fn assets_panel(store: Rc<Store>) -> View {
    let th = theme();
    let assets = store.state.assets.get();
    let query = store.state.asset_search_query.get();

    const SEARCH_KEY: u64 = 0x4153534554u64;
    let search_state = get_textfield_state(SEARCH_KEY).unwrap_or_else(|| {
        let s = Rc::new(std::cell::RefCell::new(TextFieldState::new()));
        s.borrow_mut().text = query.clone();
        set_textfield_state(SEARCH_KEY, s.clone());
        s
    });

    let search = Row(
        Modifier::new()
            .fill_max_width()
            .height(40.0)
            .background(th.surface)
            .border(1.0, th.outline, 0.0)
            .padding_values(repose_core::PaddingValues {
                left: 12.0,
                right: 12.0,
                top: 4.0,
                bottom: 4.0,
            })
            .align_items(repose_core::AlignItems::CENTER),
    )
    .child(
        BasicTextField(
            search_state.clone(),
            Modifier::new()
                .flex_grow(1.0)
                .height(32.0)
                .background(th.surface_variant.with_alpha(80))
                .border(1.0, th.outline, 8.0)
                .padding_values(repose_core::PaddingValues {
                    left: 10.0,
                    right: 10.0,
                    top: 0.0,
                    bottom: 0.0,
                }),
            "Search assets…",
            TextFieldConfig {
                on_change: Some(Rc::new({
                    let store = store.clone();
                    move |v| {
                        store.state.asset_search_query.set(v);
                    }
                })),
                ..Default::default()
            },
        )
    );

    let header = Row(
        Modifier::new()
            .fill_max_width()
            .height(36.0)
            .background(th.surface)
            .padding_values(repose_core::PaddingValues {
                left: 12.0,
                right: 12.0,
                top: 8.0,
                bottom: 8.0,
            })
            .align_items(repose_core::AlignItems::CENTER),
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

    let filtered: Vec<&Asset> = if query.is_empty() {
        assets.iter().collect()
    } else {
        let q = query.to_lowercase();
        assets
            .iter()
            .filter(|a| a.name.to_lowercase().contains(&q))
            .collect()
    };

    let list = if assets.is_empty() {
        Column(
            Modifier::new()
                .fill_max_size()
                .align_items(repose_core::AlignItems::CENTER)
                .justify_content(repose_core::AlignContent::CENTER)
                .padding(16.0),
        )
        .child((
            Text("No assets yet").size(13.0).color(th.on_surface_variant),
            Box(Modifier::new().height(6.0)),
            Text("Import media to get started.")
                .size(11.0)
                .color(th.on_surface_variant.with_alpha(160)),
        ))
    } else if filtered.is_empty() {
        Column(
            Modifier::new()
                .fill_max_size()
                .align_items(repose_core::AlignItems::CENTER)
                .justify_content(repose_core::AlignContent::CENTER)
                .padding(16.0),
        )
        .child((
            Text("No matches").size(13.0).color(th.on_surface_variant),
            Box(Modifier::new().height(6.0)),
            Text("Try a different search term.")
                .size(11.0)
                .color(th.on_surface_variant.with_alpha(160)),
        ))
    } else {
        let rows: Vec<View> = filtered
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
            .align_items(repose_core::AlignItems::CENTER),
    )
    .child((
        material3::Button(
            Modifier::new().width(180.0),
            {
                let store = store.clone();
                move || {
                    if let Some(paths) = rfd::FileDialog::new().pick_files() {
                        store.dispatch_asset(AssetCommand::Import { paths });
                    }
                }
            },
            Default::default(),
            move || Text("Import Media"),
        ),
        Spacer().modifier(Modifier::new().flex_grow(1.0)),
        Text("Tip: drag assets into the timeline")
            .size(11.0)
            .color(th.on_surface_variant),
    ));

    Column(Modifier::new().fill_max_size().background(th.background)).child((
        search,
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
        snapshort_usecases::AssetStatus::Analyzing { progress } => {
            format!("Analyzing {progress}%")
        }
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
            .align_items(repose_core::AlignItems::CENTER)
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
                .align_items(repose_core::AlignItems::CENTER)
                .justify_content(repose_core::AlignContent::CENTER),
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
            Row(Modifier::new().align_items(repose_core::AlignItems::CENTER).gap(8.0)).child((
                chip(type_label, type_tint, type_tint.with_alpha(24)),
                chip(&duration, th.on_surface_variant, th.surface_variant),
                status_widget(&asset.status, &status_label, th),
            )),
        )),
        Row(Modifier::new().align_items(repose_core::AlignItems::CENTER).gap(4.0)).child((
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
                                        masks: vec![],
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
                                    blend_mode: Default::default(),
                                };
                                store.dispatch_edit(EditCommand::AddClip { track_id, clip });
                            }
                        },
                        Default::default(),
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
                        Default::default(),
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
                Default::default(),
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
                Default::default(),
            ),
        )),
    ]);

    let captured_asset_id = asset.id;
    row.modifier(Modifier::new().clickable().on_click(move || {
        store.state.selected_asset_id.set(Some(captured_asset_id));
        store.state.selected_clip_id.set(None);
    }))
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

fn status_widget(status: &snapshort_usecases::AssetStatus, label: &str, th: repose_core::Theme) -> View {
    match status {
        snapshort_usecases::AssetStatus::Analyzing { progress } => {
            Column(Modifier::new().width(80.0).gap(2.0)).child((
                Text(label).size(9.0).color(th.on_surface_variant),
                material3::LinearProgressIndicator(Some(*progress as f32 / 100.0), Default::default())
                    .modifier(Modifier::new().height(4.0).fill_max_width()),
            ))
        }
        _ => chip(label, th.on_surface_variant, th.surface_variant),
    }
}
