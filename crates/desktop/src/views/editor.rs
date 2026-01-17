use super::{assets::assets_panel, timeline::timeline_panel};
use crate::state::Store;
use repose_core::{Color, Modifier, View};
use repose_ui::{Box, Column, Row, Text, TextStyle, ViewExt};
use snapshort_ui_core::colors;
use std::rc::Rc;

fn h_spacer(w: f32) -> View {
    Box(Modifier::new().width(w))
}

fn v_spacer(h: f32) -> View {
    Box(Modifier::new().height(h))
}

pub fn editor_screen(store: Rc<Store>) -> View {
    let center_column = Column(Modifier::new().flex_grow(1.0)).child((
        center_area(store.clone()),
        Row(Modifier::new().fill_max_width().height(350.0)).child(timeline_panel(store.clone())),
    ));

    let main_row = Row(Modifier::new().flex_grow(1.0)).child((
        left_panel(store.clone()),
        center_column,
        right_panel(store.clone()),
    ));

    Column(Modifier::new().fill_max_size()).child((
        menu_bar(store.clone()),
        main_row,
        status_bar(store),
    ))
}

fn menu_bar(_store: Rc<Store>) -> View {
    Row(Modifier::new()
        .fill_max_width()
        .height(28.0)
        .background(colors::BG_PANEL)
        .padding_values(repose_core::PaddingValues {
            left: 8.0,
            right: 8.0,
            top: 0.0,
            bottom: 0.0,
        })
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        menu_item("File"),
        menu_item("Edit"),
        menu_item("Clip"),
        menu_item("Sequence"),
        menu_item("Marker"),
        menu_item("Graphics"),
        menu_item("Window"),
        menu_item("Help"),
        Box(Modifier::new().flex_grow(1.0)),
        Text("Project Settings")
            .size(11.0)
            .color(colors::TEXT_MUTED),
    ])
}

fn menu_item(label: &str) -> View {
    Text(label)
        .size(12.0)
        .color(colors::TEXT_PRIMARY)
        .modifier(Modifier::new().padding(6.0).on_pointer_enter(|_| {}))
}

fn left_panel(store: Rc<Store>) -> View {
    let tabs = Row(Modifier::new()
        .height(28.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(8.0))
    .child((
        panel_tab("Project", true),
        h_spacer(8.0),
        panel_tab("Media Browser", false),
        h_spacer(8.0),
        panel_tab("Effects", false),
    ));

    let divider = Box(Modifier::new()
        .fill_max_width()
        .height(1.0)
        .background(colors::BORDER));

    let info_panel = Box(Modifier::new().fill_max_width().height(120.0).padding(8.0)).child((
        Text("Info Panel").size(12.0).color(colors::TEXT_MUTED),
        Text("No selection").size(11.0).color(colors::TEXT_DISABLED),
    ));

    Column(
        Modifier::new()
            .width(300.0)
            .fill_max_height()
            .background(colors::BG_PANEL)
            .border(1.0, colors::BORDER, 0.0),
    )
    .child((
        panel_header("Project"),
        tabs,
        assets_panel(store),
        divider,
        info_panel,
    ))
}

fn center_area(_store: Rc<Store>) -> View {
    let video_preview = Box(Modifier::new()
        .fill_max_width()
        .flex_grow(1.0)
        .padding(32.0)
        .background(Color::BLACK))
    .child(
        Box(Modifier::new()
            .fill_max_size()
            .background(colors::BG_DARK)
            .border(1.0, colors::BORDER, 0.0))
        .child(Text("No Video").size(14.0).color(colors::TEXT_MUTED)),
    );

    let playback_controls = Row(Modifier::new()
        .height(40.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(8.0)
        .justify_content(repose_core::JustifyContent::Center))
    .child(vec![
        playback_button("⏮"),
        h_spacer(16.0),
        playback_button("◀"),
        h_spacer(16.0),
        playback_button("▶"),
        h_spacer(16.0),
        playback_button("⏹"),
        h_spacer(16.0),
        playback_button("⏭"),
    ]);

    let program_monitor = Column(Modifier::new().fill_max_width().flex_grow(1.0).border(
        1.0,
        colors::BORDER,
        0.0,
    ))
    .child((
        panel_header("Program Monitor"),
        monitor_toolbar(),
        video_preview,
        playback_controls,
    ));

    let source_preview = Box(Modifier::new()
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
    );

    let source_monitor = Column(Modifier::new().fill_max_width().height(280.0))
        .child((panel_header("Source Monitor"), source_preview));

    Column(
        Modifier::new()
            .fill_max_width()
            .flex_grow(1.0)
            .background(colors::BG_DARK),
    )
    .child((program_monitor, source_monitor))
}

fn right_panel(store: Rc<Store>) -> View {
    let tabs = Row(Modifier::new()
        .height(28.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(8.0))
    .child((
        panel_tab("Effect Controls", true),
        h_spacer(8.0),
        panel_tab("Audio Clip Mixer", false),
        h_spacer(8.0),
        panel_tab("Metadata", false),
    ));

    let divider = Box(Modifier::new()
        .fill_max_width()
        .height(1.0)
        .background(colors::BORDER));

    let history_content = Box(Modifier::new().fill_max_width().height(200.0).padding(8.0)).child((
        Text("Project Created")
            .size(11.0)
            .color(colors::TEXT_PRIMARY),
        Text("Timeline Created")
            .size(11.0)
            .color(colors::TEXT_PRIMARY),
    ));

    let history_section =
        Column(Modifier::new().fill_max_width()).child((panel_header("History"), history_content));

    Column(
        Modifier::new()
            .width(280.0)
            .fill_max_height()
            .background(colors::BG_PANEL)
            .border(1.0, colors::BORDER, 0.0),
    )
    .child((
        panel_header("Inspector"),
        tabs,
        inspector_content(store.clone()),
        divider,
        history_section,
    ))
}

fn panel_header(title: &str) -> View {
    let buttons = Row(Modifier::new()).child((
        header_button("⁻"),
        h_spacer(4.0),
        header_button("□"),
        h_spacer(4.0),
        header_button("×"),
    ));

    Row(Modifier::new()
        .fill_max_width()
        .height(28.0)
        .background(colors::BG_HEADER)
        .border(1.0, colors::BORDER, 0.0)
        .padding_values(repose_core::PaddingValues {
            left: 12.0,
            right: 8.0,
            top: 0.0,
            bottom: 0.0,
        })
        .align_items(repose_core::AlignItems::Center))
    .child((
        Text(title).size(12.0).color(colors::TEXT_HEADER),
        Box(Modifier::new().flex_grow(1.0)),
        buttons,
    ))
}

fn panel_tab(label: &str, active: bool) -> View {
    let bg = if active {
        colors::BG_ACTIVE_TAB
    } else {
        Color::TRANSPARENT
    };
    let text_color = if active {
        colors::TEXT_PRIMARY
    } else {
        colors::TEXT_MUTED
    };

    Box(Modifier::new()
        .background(bg)
        .padding_values(repose_core::PaddingValues {
            left: 8.0,
            right: 8.0,
            top: 4.0,
            bottom: 4.0,
        })
        .border(if active { 2.0 } else { 0.0 }, colors::ACCENT, 0.0))
    .child(Text(label).size(11.0).color(text_color))
}

fn monitor_toolbar() -> View {
    let divider1 = Box(Modifier::new()
        .width(1.0)
        .height(16.0)
        .background(colors::BORDER));
    let divider2 = Box(Modifier::new()
        .width(1.0)
        .height(16.0)
        .background(colors::BORDER));

    Row(Modifier::new()
        .fill_max_width()
        .height(32.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding(8.0)
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        Text("100%").size(11.0).color(colors::TEXT_PRIMARY),
        Box(Modifier::new().flex_grow(1.0)),
        Text("Full").size(11.0).color(colors::TEXT_MUTED),
        h_spacer(12.0),
        divider1,
        h_spacer(12.0),
        Text("00:00:00:00").size(11.0).color(colors::TEXT_PRIMARY),
        h_spacer(12.0),
        divider2,
        h_spacer(12.0),
        Text("Drop Frame").size(10.0).color(colors::TEXT_MUTED),
    ])
}

fn inspector_content(_store: Rc<Store>) -> View {
    let divider1 = Box(Modifier::new()
        .fill_max_width()
        .height(1.0)
        .background(colors::BORDER));
    let divider2 = Box(Modifier::new()
        .fill_max_width()
        .height(1.0)
        .background(colors::BORDER));

    Column(Modifier::new().fill_max_width().flex_grow(1.0).padding(8.0)).child((
        inspector_section("Motion", &["Position", "Scale", "Rotation", "Anchor Point"]),
        divider1,
        v_spacer(8.0),
        inspector_section("Opacity", &["Opacity"]),
        divider2,
        v_spacer(8.0),
        inspector_section("Audio", &["Volume", "Pan"]),
    ))
}

fn inspector_section(title: &str, properties: &[&str]) -> View {
    let property_rows: Vec<View> = properties
        .iter()
        .map(|prop| {
            let value_box = Box(Modifier::new()
                .width(80.0)
                .height(20.0)
                .background(colors::BG_DARK)
                .border(1.0, colors::BORDER, 2.0)
                .padding(4.0))
            .child(Text("0").size(10.0).color(colors::TEXT_PRIMARY));

            Row(Modifier::new()
                .height(28.0)
                .padding(4.0)
                .align_items(repose_core::AlignItems::Center))
            .child((
                Text(*prop).size(11.0).color(colors::TEXT_MUTED),
                Box(Modifier::new().flex_grow(1.0)),
                value_box,
            ))
        })
        .collect();

    let header_row = Row(Modifier::new()
        .height(24.0)
        .padding(4.0)
        .align_items(repose_core::AlignItems::Center))
    .child((
        Text(title).size(11.0).color(colors::TEXT_PRIMARY),
        Box(Modifier::new().flex_grow(1.0)),
        header_button("▼"),
    ));

    let properties_column = Column(Modifier::new().padding_values(repose_core::PaddingValues {
        left: 16.0,
        right: 0.0,
        top: 0.0,
        bottom: 0.0,
    }))
    .child(property_rows);

    Column(Modifier::new().fill_max_width()).child((header_row, properties_column))
}

fn header_button(icon: &str) -> View {
    Box(Modifier::new()
        .width(20.0)
        .height(20.0)
        .padding(2.0)
        .on_pointer_enter(|_| {}))
    .child(Text(icon).size(12.0).color(colors::TEXT_MUTED))
}

fn playback_button(icon: &str) -> View {
    playback_button_with_color(icon, colors::TEXT_PRIMARY)
}

fn playback_button_with_color(icon: &str, color: Color) -> View {
    Box(Modifier::new()
        .width(32.0)
        .height(32.0)
        .on_pointer_enter(|_| {}))
    .child(Text(icon).size(18.0).color(color))
}

fn status_bar(store: Rc<Store>) -> View {
    let project_name = store
        .state
        .project
        .get()
        .map(|p| p.name.clone())
        .unwrap_or("No Project".to_string());
    let msg = store.state.status_msg.get();

    let divider1 = Box(Modifier::new()
        .width(1.0)
        .height(12.0)
        .background(colors::BORDER));
    let divider2 = Box(Modifier::new()
        .width(1.0)
        .height(12.0)
        .background(colors::BORDER));

    Row(Modifier::new()
        .fill_max_width()
        .height(24.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding_values(repose_core::PaddingValues {
            left: 8.0,
            right: 8.0,
            top: 0.0,
            bottom: 0.0,
        })
        .align_items(repose_core::AlignItems::Center))
    .child(vec![
        Text(project_name).size(11.0).color(colors::TEXT_MUTED),
        h_spacer(8.0),
        divider1,
        h_spacer(8.0),
        Text("Sequence: Sequence 01 | 1920x1080 | 24fps")
            .size(11.0)
            .color(colors::TEXT_MUTED),
        Box(Modifier::new().flex_grow(1.0)),
        Text(msg).size(11.0).color(colors::TEXT_ACCENT),
        h_spacer(8.0),
        divider2,
        h_spacer(8.0),
        Text("Ready").size(11.0).color(colors::TEXT_MUTED),
    ])
}
