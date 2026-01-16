use crate::state::Store;
use repose_core::{view::View, Color, Modifier};
use repose_ui::{
    scroll::{remember_scroll_state, ScrollArea},
    Box, Column, Row, Spacer, Text, TextStyle, ViewExt,
};

use snapshort_ui_core::{colors, primary_button};
use snapshort_usecases::AssetCommand;

use std::rc::Rc;

pub fn assets_panel(store: Rc<Store>) -> View {
    let assets = store.state.assets.get();

    let asset_rows: Vec<View> = assets
        .iter()
        .enumerate()
        .map(|(idx, asset)| asset_item(asset, idx))
        .collect();

    Column(Modifier::new().fill_max_size().background(colors::BG_DARK))
        // Header row
        .child(
            Row(Modifier::new()
                .fill_max_width()
                .height(24.0)
                .background(colors::BG_PANEL)
                .border(1.0, colors::BORDER, 0.0)
                .padding(8.0)
                .align_items(repose_core::AlignItems::Center))
            .child((
                Text("Name").size(10.0).color(colors::TEXT_MUTED),
                Box(Modifier::new().flex_grow(1.0)),
                Text("Type").size(10.0).color(colors::TEXT_MUTED),
                Box(Modifier::new().width(8.0)),
                Text("Status").size(10.0).color(colors::TEXT_MUTED),
            )),
        )
        // List
        .child(
            Row(Modifier::new().flex_grow(1.0)).child(if assets.is_empty() {
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
                ScrollArea(
                    Modifier::new().fill_max_size(),
                    remember_scroll_state("assets_list"),
                    Column(Modifier::new().fill_max_width()).child(asset_rows),
                )
            }),
        )
        // Footer: Import button
        .child(
            Row(Modifier::new()
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
            )),
        )
}

fn asset_item(asset: &snapshort_domain::Asset, idx: usize) -> View {
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

    Row(Modifier::new()
        .key(idx as u64)
        .fill_max_width()
        .height(32.0)
        .padding(8.0)
        .align_items(repose_core::AlignItems::Center))
    .child((
        Box(Modifier::new().width(16.0).height(16.0)).child(Text(icon).size(12.0)),
        Text(asset.name.clone())
            .size(11.0)
            .color(colors::TEXT_PRIMARY),
        Box(Modifier::new().flex_grow(1.0)),
        Text(type_label).size(10.0).color(color),
        Box(Modifier::new().width(8.0)),
        Text(status).size(10.0).color(colors::TEXT_MUTED),
    ))
}
