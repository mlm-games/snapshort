use crate::state::Store;
use repose_core::{view::View, Color, Modifier};
use repose_ui::{
    scroll::{remember_scroll_state, ScrollArea},
    Box, Button, Column, Row, Spacer, Text, TextStyle, ViewExt,
};
use snapshort_domain::{TrackRef, TrackType};
use snapshort_ui_core::{colors, icon_button, primary_button};
use snapshort_usecases::{AssetCommand, TimelineCommand};
use std::rc::Rc;

pub fn assets_panel(store: Rc<Store>) -> View {
    let assets = store.state.assets.get();

    let header = Row(Modifier::new()
        .fill_max_width()
        .height(24.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(8.0)
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        Text("Name").size(10.0).color(colors::TEXT_MUTED),
        Box(Modifier::new().flex_grow(1.0)),
        Text("Type").size(10.0).color(colors::TEXT_MUTED),
        Box(Modifier::new().width(8.0)),
        Text("Dur").size(10.0).color(colors::TEXT_MUTED),
        Box(Modifier::new().width(8.0)),
        Text("Status").size(10.0).color(colors::TEXT_MUTED),
        Box(Modifier::new().width(8.0)),
        Text("Actions").size(10.0).color(colors::TEXT_MUTED),
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
            Text("No assets yet").size(12.0).color(colors::TEXT_MUTED),
            Text("Import media to get started")
                .size(11.0)
                .color(colors::TEXT_DISABLED),
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

    let footer = Row(Modifier::new()
        .fill_max_width()
        .height(52.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(8.0)
        .align_items(repose_core::AlignItems::Center))
    .child((
        primary_button("Import Media", {
            let store = store.clone();
            move || {
                if let Some(paths) = rfd::FileDialog::new().pick_files() {
                    store.dispatch_asset(AssetCommand::Import { paths });
                }
            }
        })
        .modifier(Modifier::new().width(200.0)),
        Spacer().modifier(Modifier::new().flex_grow(1.0)),
        Text("Tip: select multiple files")
            .size(10.0)
            .color(colors::TEXT_MUTED),
    ));

    Column(Modifier::new().fill_max_size().background(colors::BG_DARK)).child((
        header,
        Row(Modifier::new().flex_grow(1.0)).child(list),
        footer,
    ))
}

fn asset_item(asset: &snapshort_domain::Asset, idx: usize, store: Rc<Store>) -> View {
    let (icon, type_label, color) = match asset.asset_type {
        snapshort_domain::AssetType::Video => ("🎬", "Video", Color(74, 144, 226, 255)),
        snapshort_domain::AssetType::Audio => ("🎵", "Audio", Color(82, 190, 128, 255)),
        snapshort_domain::AssetType::Image => ("📷", "Image", Color(243, 156, 18, 255)),
        snapshort_domain::AssetType::Sequence => ("🎞️", "Seq", Color(155, 89, 182, 255)),
    };

    let status = match &asset.status {
        snapshort_domain::AssetStatus::Pending => "pending".to_string(),
        snapshort_domain::AssetStatus::Analyzing => "analyzing".to_string(),
        snapshort_domain::AssetStatus::Ready => "ready".to_string(),
        snapshort_domain::AssetStatus::ProxyGenerating { progress } => {
            format!("proxy {}%", progress)
        }
        snapshort_domain::AssetStatus::ProxyReady => "proxy ready".to_string(),
        snapshort_domain::AssetStatus::Offline => "offline".to_string(),
        snapshort_domain::AssetStatus::Error(e) => format!("error: {}", e),
    };

    let duration = asset
        .media_info
        .as_ref()
        .map(|m| format!("{:.1}s", (m.duration_ms as f64) / 1000.0))
        .unwrap_or_else(|| "-".to_string());

    let selected = store.state.selected_asset_id.get() == Some(asset.id);
    let bg = if selected {
        colors::BG_SELECTED
    } else {
        colors::BG_DARK
    };
    let border = if selected {
        colors::ACCENT
    } else {
        colors::BORDER
    };

    let row = Row(Modifier::new()
        .key(idx as u64)
        .fill_max_width()
        .height(34.0)
        .padding(8.0)
        .align_items(repose_core::AlignItems::Center)
        .background(bg)
        .border(1.0, border, 0.0))
    .child(vec![
        Box(Modifier::new().width(16.0).height(16.0)).child(Text(icon).size(12.0)),
        Text(asset.name.clone())
            .size(11.0)
            .color(colors::TEXT_PRIMARY),
        Box(Modifier::new().flex_grow(1.0)),
        Text(type_label).size(10.0).color(color),
        Box(Modifier::new().width(8.0)),
        Text(duration).size(10.0).color(colors::TEXT_MUTED),
        Box(Modifier::new().width(8.0)),
        Text(status).size(10.0).color(colors::TEXT_MUTED),
        Box(Modifier::new().width(8.0)),
        Row(Modifier::new().align_items(repose_core::AlignItems::Center)).child((
            // Add to timeline at end on V1
            icon_button("➕", {
                let store = store.clone();
                let asset_id = asset.id;
                move || {
                    if let Some(tl) = store.state.timeline.get() {
                        let start = tl.duration(); // Frame
                        store.dispatch_timeline(TimelineCommand::InsertClip {
                            asset_id,
                            timeline_start: start,
                            track: TrackRef::video(0),
                            source_range: None,
                        });
                    }
                }
            })
            .modifier(Modifier::new().padding(2.0)),
            // Generate proxy
            icon_button("⚡", {
                let store = store.clone();
                let asset_id = asset.id;
                move || store.dispatch_asset(AssetCommand::GenerateProxy { asset_id })
            })
            .modifier(Modifier::new().padding(2.0)),
            // Delete
            icon_button("🗑", {
                let store = store.clone();
                let asset_id = asset.id;
                move || store.dispatch_asset(AssetCommand::Delete { asset_id })
            })
            .modifier(Modifier::new().padding(2.0)),
        )),
    ]);

    // Make the whole row clickable for selection
    Button(row, {
        let store = store.clone();
        let asset_id = asset.id;
        move || {
            store.state.selected_asset_id.set(Some(asset_id));
            store.state.selected_clip_id.set(None);
        }
    })
}
