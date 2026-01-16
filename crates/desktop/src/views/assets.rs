use crate::state::Store;
use repose_core::{Color, Modifier, View};
use repose_ui::{Box, Column, Row, Text, TextStyle, ViewExt};
use snapshort_ui_core::colors;
use std::rc::Rc;

pub fn assets_panel(store: Rc<Store>) -> View {
    let assets = store.state.assets.get();

    // IMPORTANT: collect iterator -> Vec<View> so it implements IntoChildren
    let asset_rows: Vec<View> = assets
        .iter()
        .enumerate()
        .map(|(idx, asset)| asset_item(asset, idx, store.clone()))
        .collect();

    Column(Modifier::new().fill_max_size().background(colors::BG_DARK))
        // Search/filter bar
        .child(
            Row(Modifier::new()
                .fill_max_width()
                .height(32.0)
                .background(colors::BG_PANEL)
                .border(1.0, colors::BORDER, 0.0)
                .padding(8.0)
                .align_items(repose_core::AlignItems::Center))
            .child(
                Box(Modifier::new()
                    .flex_grow(1.0)
                    .height(20.0)
                    .background(colors::BG_DARK)
                    .border(1.0, colors::BORDER, 2.0)
                    .padding(4.0))
                .child(
                    Text("🔍 Search assets")
                        .size(10.0)
                        .color(colors::TEXT_MUTED),
                ),
            )
            .child(
                Box(Modifier::new()
                    .width(24.0)
                    .height(20.0)
                    .background(colors::BG_DARK)
                    .border(1.0, colors::BORDER, 2.0))
                .child(Text("⚙").size(10.0).color(colors::TEXT_MUTED)),
            ),
        )
        // Asset list header
        .child(
            Row(Modifier::new()
                .fill_max_width()
                .height(24.0)
                .background(colors::BG_PANEL)
                .border(1.0, colors::BORDER, 0.0)
                .padding(8.0))
            .child((
                Text("Name").size(10.0).color(colors::TEXT_MUTED),
                Box(Modifier::new().flex_grow(1.0)),
                Text("Type").size(10.0).color(colors::TEXT_MUTED),
                Box(Modifier::new().width(8.0)),
                Text("Dur").size(10.0).color(colors::TEXT_MUTED),
            )),
        )
        // Asset list
        .child(
            Row(Modifier::new().flex_grow(1.0)).child(if assets.is_empty() {
                // Empty state
                Box(Modifier::new()
                    .fill_max_width()
                    .height(200.0)
                    .align_items(repose_core::AlignItems::Center)
                    .justify_content(repose_core::JustifyContent::Center)
                    .padding(16.0))
                .child((
                    Text("No assets yet").size(12.0).color(colors::TEXT_MUTED),
                    Text("Import media to get started")
                        .size(10.0)
                        .color(colors::TEXT_DISABLED),
                ))
            } else {
                Column(Modifier::new().fill_max_width()).child(asset_rows)
            }),
        )
        // Import button area
        .child(
            Box(Modifier::new()
                .fill_max_width()
                .height(40.0)
                .background(colors::BG_PANEL)
                .border(1.0, colors::BORDER, 0.0)
                .padding(8.0)
                .on_pointer_enter(|_| {}))
            .child(
                Row(Modifier::new()
                    .align_items(repose_core::AlignItems::Center)
                    .justify_content(repose_core::JustifyContent::Center))
                .child((
                    Text("📁").size(14.0).color(colors::TEXT_ACCENT),
                    Box(Modifier::new().width(8.0)),
                    Text("Import Media").size(11.0).color(colors::TEXT_ACCENT),
                )),
            ),
        )
}

fn asset_item(asset: &snapshort_domain::Asset, idx: usize, _store: Rc<Store>) -> View {
    let name = &asset.name;

    let (icon, type_label, color) = match asset.asset_type {
        snapshort_domain::AssetType::Video => ("🎬", "Video", colors::VIDEO_TRACK),
        snapshort_domain::AssetType::Audio => ("🎵", "Audio", colors::AUDIO_TRACK),
        snapshort_domain::AssetType::Image => ("📷", "Image", Color(243, 156, 18, 255)),
        snapshort_domain::AssetType::Sequence => ("🎞️", "Seq", Color(155, 89, 182, 255)),
        _ => ("📄", "Other", colors::TEXT_MUTED),
    };

    let is_selected = false; // TODO

    Box(Modifier::new()
        // Stable identity hint for dynamic lists (optional but nice)
        .key(idx as u64)
        .fill_max_width()
        .height(32.0)
        .background(if is_selected {
            colors::BG_SELECTED
        } else {
            Color::TRANSPARENT
        })
        .on_pointer_enter(|_| {})
        .padding(8.0))
    .child(
        Row(Modifier::new()
            .fill_max_width()
            .align_items(repose_core::AlignItems::Center))
        .child((
            Box(Modifier::new().width(16.0).height(16.0)).child(Text(icon).size(12.0)),
            Text(name).size(11.0).color(if is_selected {
                Color::WHITE
            } else {
                colors::TEXT_PRIMARY
            }),
            Box(Modifier::new().flex_grow(1.0)),
            Text(type_label).size(10.0).color(if is_selected {
                Color::WHITE
            } else {
                colors::TEXT_MUTED
            }),
            Box(Modifier::new().width(8.0)),
            Box(Modifier::new()
                .width(40.0)
                .height(16.0)
                .border(1.0, color, 0.0)
                .padding(2.0))
            .child(Text("00:10").size(9.0).color(if is_selected {
                Color::WHITE
            } else {
                colors::TEXT_MUTED
            })),
            // FIX: on_pointer_enter expects a closure, not a Modifier
            Box(Modifier::new()
                .width(20.0)
                .height(20.0)
                .on_pointer_enter(|_| {}))
            .child(Text("⋯").size(12.0).color(colors::TEXT_MUTED)),
        )),
    )
}
