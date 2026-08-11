//! Timeline panel: geometry-driven tracks and clips, Miniter-style ruler,
//! playhead and snap guide, plus compact headers and context menus.

pub mod clip;
pub mod geometry;
pub mod menus;
pub mod ruler;
pub mod track;

use crate::state::Store;
use geometry::{
    ADD_TRACK_ROW_HEIGHT, RULER_HEIGHT, TRACK_HEADER_WIDTH, TRACK_HEIGHT, TimelineScale,
    timeline_width,
};
use miniter_domain::{Track, TrackKind};
use menus::{add_track_menu_items, clip_menu_items, popover_view, track_menu_items};
use repose_core::{Modifier, Vec2, View};
use repose_ui::scroll::{remember_scroll_state, remember_scroll_state_xy, ScrollArea, ScrollAreaXY};
use repose_ui::{Box, Column, Row, Text, TextStyle, ViewExt};
use snapshort_ui_core::colors;
use std::rc::Rc;

fn h_spacer(w: f32) -> View {
    Box(Modifier::new().width(w))
}

pub fn timeline_panel(store: Rc<Store>) -> View {
    let timeline = store.state.timeline.get();
    let zoom = store.state.timeline_zoom.get();
    let scale = TimelineScale::new(zoom);

    let header_scroll = remember_scroll_state("timeline_headers_y");
    let body_scroll = remember_scroll_state_xy("timeline_tracks_xy");
    let (scroll_x, scroll_y) = body_scroll.get();
    header_scroll.set_offset(scroll_y);

    if store.state.playback_state.get() == "Playing" && timeline.is_some() {
        let playhead_px = scale.us_to_x(store.state.playhead.get().0);
        let vp_w_est = 700.0;
        let margin = vp_w_est * 0.33;
        let target = playhead_px - margin;
        if target > scroll_x + 20.0 || target < scroll_x - vp_w_est * 0.5 {
            body_scroll.set_offset_xy(target.max(0.0), scroll_y);
        }
    }

    let store_for_origin = store.clone();
    let origin_box = Box(Modifier::new()
        .absolute()
        .offset(Some(0.0), Some(0.0), None, None)
        .width(1.0)
        .height(1.0)
        .on_globally_positioned(move |rect| {
            *store_for_origin.state.panel_origin.borrow_mut() = Some(Vec2 {
                x: rect.x,
                y: rect.y,
            });
        }));

    let panel_origin = match *store.state.panel_origin.borrow() {
        Some(o) => o,
        None => Vec2 { x: 0.0, y: 0.0 },
    };

    let tracks: Vec<&Track> = timeline
        .as_ref()
        .map(|tl| tl.tracks.iter().collect())
        .unwrap_or_default();

    let content_w = timeline_width(timeline.as_ref(), scale);

    // Header column: empty ruler-corner spacer, then compact track headers.
    let mut header_views: Vec<View> = Vec::new();
    header_views.push(Box(Modifier::new()
        .width(TRACK_HEADER_WIDTH)
        .height(RULER_HEIGHT)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)));

    for track in &tracks {
        header_views.push(track::track_header(store.clone(), track));
    }
    header_views.push(Box(Modifier::new()
        .width(TRACK_HEADER_WIDTH)
        .height(ADD_TRACK_ROW_HEIGHT)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)));

    // Content column: fixed ruler, lanes, and the add-track row.
    let mut content_views: Vec<View> = Vec::new();
    content_views.push(ruler::ruler_row(
        store.clone(),
        body_scroll.clone(),
        scale,
    ));

    if tracks.is_empty() {
        content_views.push(empty_lane(TrackKind::Video, content_w));
        content_views.push(empty_lane(TrackKind::Audio, content_w));
    } else {
        for track in tracks {
            content_views.push(track::track_lane(
                store.clone(),
                track,
                scale,
                body_scroll.clone(),
                panel_origin,
            ));
        }
    }
    content_views.push(track::add_track_row(store.clone()));

    let toolbar = timeline_toolbar(scale);

    let header_pane = ScrollArea(
        Modifier::new().width(TRACK_HEADER_WIDTH).fill_max_height(),
        header_scroll,
        Column(Modifier::new().width(TRACK_HEADER_WIDTH)).child(header_views),
    );

    let body_pane = ScrollAreaXY(
        Modifier::new().fill_max_size(),
        body_scroll.clone(),
        Column(Modifier::new().width(content_w.max(1.0))).child(content_views),
    );

    let playhead_x = scale.us_to_x(store.state.playhead.get().0) - scroll_x;
    let playhead_overlay = repose_canvas::Canvas(
        Modifier::new().fill_max_height().width(2.0),
        move |scope: &mut repose_canvas::DrawScope| {
            let height = scope.size.height;
            scope.draw_rect_stroke(
                repose_core::Rect { x: 0.0, y: 0.0, w: 1.0, h: height },
                colors::PLAYHEAD,
                0.0,
                1.0,
            );
        },
    )
    .modifier(Modifier::new()
        .width(2.0)
        .fill_max_height()
        .absolute()
        .offset(Some(playhead_x), Some(0.0), None, None)
        .z_index(90.0)
        .hit_passthrough());

    let content = Row(Modifier::new().fill_max_size().flex_grow(1.0)).child((
        header_pane,
        Column(Modifier::new().fill_max_size().flex_grow(1.0)).child((
            body_pane,
            playhead_overlay,
        )),
    ));

    let main = Column(Modifier::new().fill_max_size().background(colors::BG_DARK)).child((
        toolbar,
        content,
    ));

    // Context menu popovers, anchored in the overlay layer.
    let mut overlays: Vec<View> = Vec::new();
    overlays.push(popover_view(
        store.overlay.clone(),
        &store.state.add_track_menu,
        add_track_menu_items(&store),
    ));

    if let Some((clip_id, track_id)) = store.state.clip_menu_target.get() {
        if let Some(clip) = timeline
            .as_ref()
            .and_then(|tl| tl.tracks.iter().find_map(|t| t.clip_by_id(clip_id)))
        {
            overlays.push(popover_view(
                store.overlay.clone(),
                &store.state.clip_menu,
                clip_menu_items(&store, clip, track_id),
            ));
        }
    }

    if let Some(track_id) = store.state.track_menu_target.get() {
        if let Some(track) = timeline.as_ref().and_then(|tl| tl.track(track_id)) {
            overlays.push(popover_view(
                store.overlay.clone(),
                &store.state.track_menu,
                track_menu_items(&store, track),
            ));
        }
    }

    Column(Modifier::new().fill_max_size()).child((
        Box(Modifier::new().fill_max_size()).child(main),
        origin_box,
        Box(Modifier::new().fill_max_size()).child(Column(Modifier::new().fill_max_size()).child(overlays)),
    ))
}

fn empty_lane(kind: TrackKind, content_w: f32) -> View {
    Box(Modifier::new()
        .width(content_w)
        .height(TRACK_HEIGHT)
        .background(colors::BG_TRACK)
        .border(1.0, colors::BORDER, 0.0)
        .align_items(repose_core::AlignItems::CENTER)
        .justify_content(repose_core::AlignContent::CENTER))
    .child(
        Text(match kind {
            TrackKind::Video => "No clips (V)",
            TrackKind::Audio => "No clips (A)",
            _ => "No clips",
        })
        .size(10.0)
        .color(colors::TEXT_DISABLED),
    )
}

fn timeline_toolbar(scale: TimelineScale) -> View {
    Row(Modifier::new()
        .fill_max_width()
        .height(30.0)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .padding_values(repose_core::PaddingValues { left: 10.0, right: 10.0, top: 4.0, bottom: 4.0 })
        .align_items(repose_core::AlignItems::CENTER))
    .child((
        Text("Timeline").size(12.0).color(colors::TEXT_PRIMARY).single_line(),
        h_spacer(8.0),
        Text(format!("{} px/s", scale.zoom))
            .size(10.0)
            .color(colors::TEXT_MUTED)
            .single_line(),
        Box(Modifier::new().flex_grow(1.0)),
        Text("Right-click a clip or track for actions")
            .size(10.0)
            .color(colors::TEXT_DISABLED)
            .single_line(),
    ))
}