//! Timeline panel: fixed ruler above a vertically-scrolling track list, with
//! geometry-driven clips, Miniter-style playhead and snap guide, compact
//! headers and context menus.

pub mod clip;
pub mod geometry;
pub mod menus;
pub mod ruler;
pub mod track;

use crate::state::Store;
use geometry::{timeline_width, TimelineScale, ADD_TRACK_ROW_HEIGHT, TRACK_HEADER_WIDTH};
use menus::{add_track_menu_items, clip_menu_items, popover_view, track_menu_items};
use miniter_domain::{ClipId, ClipKind, Timestamp, Track};
use repose_core::{Modifier, Vec2, View};
use repose_ui::scroll::{
    remember_scroll_state, remember_scroll_state_xy, ScrollArea, ScrollAreaXY,
};
use repose_ui::{Box, Column, Row, Text, TextStyle, ViewExt, ZStack};
use snapshort_ui_core::colors;
use std::rc::Rc;

/// Single source of truth for whether a clip can be split at the playhead.
pub fn can_split_clip(
    timeline: &miniter_domain::Timeline,
    clip_id: ClipId,
    playhead: Timestamp,
) -> bool {
    let Some((clip, track)) = timeline
        .tracks
        .iter()
        .find_map(|t| t.clip_by_id(clip_id).map(|c| (c, t)))
    else {
        return false;
    };

    !track.locked
        && matches!(clip.kind, ClipKind::Video(_))
        && playhead.0 > clip.timeline_start.0
        && playhead.0 < clip.timeline_end().as_micros()
}

pub fn timeline_panel(store: Rc<Store>) -> View {
    let timeline = store.state.timeline.get();
    let zoom = store.state.timeline_zoom.get();
    let scale = TimelineScale::new(zoom);

    let Some(timeline_ref) = timeline.as_ref() else {
        return empty_state("No project loaded");
    };

    let header_scroll = remember_scroll_state("timeline_headers_y");
    let body_scroll = remember_scroll_state_xy("timeline_tracks_xy");
    let (scroll_x, scroll_y) = body_scroll.get();
    header_scroll.set_offset(scroll_y);

    if store.state.playback_state.get() == "Playing" {
        let playhead_px = scale.us_to_x(store.state.playhead.get().0);
        let vp_w = body_scroll.viewport().0.max(200.0);
        let margin = vp_w * 0.33;
        let target = playhead_px - margin;
        if target > scroll_x + 20.0 || target < scroll_x - vp_w * 0.5 {
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

    let tracks: Vec<&Track> = timeline_ref.tracks.iter().collect();

    let content_w = timeline_width(Some(timeline_ref), scale);

    // Header column: track headers, then the Miniter-style add-track `+` cell.
    let mut header_views: Vec<View> = Vec::new();
    for track in &tracks {
        header_views.push(track::track_header(store.clone(), track));
    }
    header_views.push(track::add_track_header_cell(store.clone()));

    // Content column: real lanes only, plus a blank full-width add-track row.
    let mut content_views: Vec<View> = Vec::new();
    for track in &tracks {
        content_views.push(track::track_lane(
            store.clone(),
            track,
            scale,
            body_scroll.clone(),
            panel_origin,
        ));
    }
    content_views.push(Box(Modifier::new()
        .fill_max_width()
        .height(ADD_TRACK_ROW_HEIGHT)
        .background(colors::BG_TRACK)
        .border(1.0, colors::BORDER, 0.0)));

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

    // Fixed ruler row above the body, sharing only horizontal scroll.
    let ruler = ruler::ruler_row(store.clone(), body_scroll.clone(), scale, panel_origin);

    let body_stack = ZStack(Modifier::new().fill_max_size().flex_grow(1.0)).child((
        body_pane,
        playhead_overlay(store.clone(), scale, scroll_x),
        snap_guide_overlay(store.clone(), scale, scroll_x),
    ));

    let content =
        Row(Modifier::new().fill_max_size().flex_grow(1.0)).child((header_pane, body_stack));

    let main =
        Column(Modifier::new().fill_max_size().background(colors::BG_DARK)).child((ruler, content));

    // Context menu popovers, anchored in the overlay layer.
    let mut overlays: Vec<View> = Vec::new();
    overlays.push(popover_view(
        store.overlay.clone(),
        &store.state.add_track_menu,
        add_track_menu_items(&store),
    ));

    if let Some((clip_id, track_id)) = store.state.clip_menu_target.get() {
        if let Some(clip) = timeline_ref
            .tracks
            .iter()
            .find_map(|t| t.clip_by_id(clip_id))
        {
            overlays.push(popover_view(
                store.overlay.clone(),
                &store.state.clip_menu,
                clip_menu_items(&store, clip, track_id),
            ));
        }
    }

    if let Some(track_id) = store.state.track_menu_target.get() {
        if let Some(track) = timeline_ref.track(track_id) {
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
        Box(Modifier::new().fill_max_size())
            .child(Column(Modifier::new().fill_max_size()).child(overlays)),
    ))
}

/// Full-height playhead line drawn over the body pane (lanes only), in the
/// same coordinate space as the ruler head.
fn playhead_overlay(store: Rc<Store>, scale: TimelineScale, scroll_x: f32) -> View {
    let playhead_x = scale.us_to_x(store.state.playhead.get().0) - scroll_x;

    repose_canvas::Canvas(
        Modifier::new().fill_max_height().width(2.0),
        move |scope: &mut repose_canvas::DrawScope| {
            let height = scope.size.height;
            scope.draw_rect(
                repose_core::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 2.0,
                    h: height,
                },
                colors::PLAYHEAD,
                0.0,
            );
        },
    )
    .modifier(
        Modifier::new()
            .width(2.0)
            .fill_max_height()
            .absolute()
            .offset(Some(playhead_x - 1.0), Some(0.0), None, None)
            .z_index(90.0)
            .hit_passthrough(),
    )
}

/// Dashed cyan snap guide, full lane-stack height, drawn while dragging.
fn snap_guide_overlay(store: Rc<Store>, scale: TimelineScale, scroll_x: f32) -> View {
    let Some(guide_us) = store.state.timeline_snap_indicator.get() else {
        return Box(Modifier::new().width(1.0).height(1.0).hit_passthrough());
    };

    let x = scale.timestamp_to_x(guide_us) - scroll_x;

    repose_canvas::Canvas(
        Modifier::new().fill_max_height().width(1.0),
        move |scope: &mut repose_canvas::DrawScope| {
            let height = scope.size.height;
            // Dashed vertical line: 6px on, 4px off.
            let mut y = 0.0;
            while y < height {
                let h = (height - y).min(6.0);
                scope.draw_rect(
                    repose_core::Rect {
                        x: 0.0,
                        y,
                        w: 1.0,
                        h,
                    },
                    colors::ACCENT_CYAN,
                    0.0,
                );
                y += 10.0;
            }
        },
    )
    .modifier(
        Modifier::new()
            .width(1.0)
            .fill_max_height()
            .absolute()
            .offset(Some(x), Some(0.0), None, None)
            .z_index(85.0)
            .hit_passthrough(),
    )
}

fn empty_state(message: &str) -> View {
    Box(Modifier::new()
        .fill_max_size()
        .background(colors::BG_DARK)
        .align_items(repose_core::AlignItems::CENTER)
        .justify_content(repose_core::AlignContent::CENTER))
    .child(Text(message).size(12.0).color(colors::TEXT_DISABLED))
}
