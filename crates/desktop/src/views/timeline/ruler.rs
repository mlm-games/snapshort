//! Fixed ruler row: time ticks, markers, and the playhead head.

use crate::state::{Store, TimelineMarker};
use super::geometry::{
    RULER_HEIGHT, TimelineScale, format_ruler_time, timeline_duration_us, timeline_width,
};
use miniter_domain::Timestamp;
use repose_core::{Modifier, Vec2, View, input::{PointerButton, PointerEventKind}};
use repose_ui::{Box, Column, Text, TextStyle, ViewExt, scroll::ScrollStateXY};
use snapshort_ui_core::colors;
use snapshort_usecases::PlaybackCommand;
use std::rc::Rc;

pub fn ruler_row(
    store: Rc<Store>,
    scroll_state_xy: Rc<ScrollStateXY>,
    scale: TimelineScale,
) -> View {
    let timeline = store.state.timeline.get();
    let total_us = timeline_duration_us(timeline.as_ref());
    let total_w = timeline_width(timeline.as_ref(), scale);
    // Horizontal scroll is used only to cull which ticks are built; the ruler
    // itself is scroll content, so children live in content coordinates.
    let scroll_x = scroll_state_xy.get().0;

    let mut children: Vec<View> = Vec::new();

    let major_us = scale.major_tick_us();
    let px_per_tick = major_us as f32 * scale.px_per_us;
    let first_label = ((scroll_x / px_per_tick.max(0.001)).floor() as i64).max(0);
    let last_label = ((scroll_x + 2000.0) / px_per_tick.max(0.001)).ceil() as i64;

    for i in first_label..=last_label {
        let us = i * major_us;
        if us > total_us {
            break;
        }
        let x = us as f32 * scale.px_per_us;
        let label = format_ruler_time(us);
        children.push(
            Box(Modifier::new()
                .absolute()
                .offset(Some(x), Some(4.0), None, None)
                .width(1.0)
                .height(6.0)
                .background(colors::TEXT_MUTED)),
        );
        children.push(
            Box(Modifier::new()
                .absolute()
                .offset(Some(x + 4.0), Some(10.0), None, None)
                .z_index(2.0))
            .child(Text(label).size(8.0).color(colors::TEXT_MUTED).single_line()),
        );
    }

    for m in store.state.timeline_markers.get() {
        children.push(marker_view(store.clone(), m, scale));
    }

    children.push(snap_guide_view(store.clone(), scale));
    children.push(playhead_head_view(store.clone(), scale));

    let store_for_seek = store.clone();
    let scroll_for_seek = scroll_state_xy.clone();
    let store_for_scroll = store.clone();

    Box(Modifier::new()
        .width(1.0)
        .height(RULER_HEIGHT)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0)
        .on_pointer_down(move |event| {
            let scroll_x = scroll_for_seek.get().0;
            let us = scale.x_to_us(event.position.x + scroll_x);
            store_for_seek.dispatch_playback(PlaybackCommand::Seek {
                timestamp: Timestamp(us.max(0)),
            });
        })
        .on_scroll(move |delta| {
            if delta.y.abs() > delta.x.abs() {
                let factor = if delta.y > 0.0 { 1.1 } else { 1.0 / 1.1 };
                let current = store_for_scroll.state.timeline_zoom.get();
                store_for_scroll
                    .state
                    .timeline_zoom
                    .set((current * factor).clamp(0.5, 20.0));
            }
            Vec2::default()
        }))
    .child(Column(Modifier::new().size(total_w, RULER_HEIGHT)).child(children))
}

fn marker_view(store: Rc<Store>, m: TimelineMarker, scale: TimelineScale) -> View {
    let x = scale.us_to_x(m.timestamp_us);
    let ts = m.timestamp_us;
    let label = m.label.clone();
    let store_for_mk = store.clone();

    Column(Modifier::new()
        .absolute()
        .offset(Some(x), Some(0.0), None, None)
        .width(80.0)
        .height(RULER_HEIGHT)
        .z_index(10.0)
        .hit_passthrough())
    .child((
        Box(Modifier::new()
            .absolute()
            .offset(Some(0.0), Some(0.0), None, None)
            .width(8.0)
            .height(10.0)
            .background(colors::MARKER)
            .on_pointer_down(move |event| {
                match &event.event {
                    PointerEventKind::Down(PointerButton::Secondary) => {
                        let mut list = store_for_mk.state.timeline_markers.get();
                        list.retain(|mk| mk.timestamp_us != ts);
                        store_for_mk.state.timeline_markers.set(list);
                    }
                    _ => {
                        store_for_mk.dispatch_playback(PlaybackCommand::Seek {
                            timestamp: Timestamp(ts),
                        });
                    }
                }
            })),
        Box(Modifier::new()
            .absolute()
            .offset(Some(10.0), Some(0.0), None, None))
        .child(Text(&label).size(8.0).color(colors::MARKER).single_line()),
    ))
}

/// Cyan dashed guide line for snap feedback, drawn in content coordinates so it
/// scrolls with the ruler + lanes.
fn snap_guide_view(store: Rc<Store>, scale: TimelineScale) -> View {
    let Some(guide_us) = store.state.timeline_snap_indicator.get() else {
        return Box(Modifier::new().width(1.0).height(1.0).hit_passthrough());
    };

    let x = scale.timestamp_to_x(guide_us);

    repose_canvas::Canvas(
        Modifier::new()
            .fill_max_size()
            .z_index(80.0)
            .hit_passthrough(),
        move |scope: &mut repose_canvas::DrawScope| {
            let height = scope.size.height;
            scope.draw_rect_stroke(
                repose_core::Rect { x, y: 0.0, w: 1.0, h: height },
                colors::ACCENT_CYAN,
                0.0,
                1.0,
            );
        },
    )
}

pub fn playhead_head_view(store: Rc<Store>, scale: TimelineScale) -> View {
    let x = scale.us_to_x(store.state.playhead.get().0);

    repose_canvas::Canvas(
        Modifier::new().fill_max_height().width(10.0),
        move |scope: &mut repose_canvas::DrawScope| {
            let height = scope.size.height;
            let width = scope.size.width;
            scope.draw_rect_stroke(
                repose_core::Rect { x: width / 2.0 - 0.5, y: 0.0, w: 1.0, h: height },
                colors::PLAYHEAD,
                0.0,
                1.0,
            );
            scope.draw_circle(
                repose_core::Vec2 { x: width / 2.0, y: 4.0 },
                4.0,
                colors::PLAYHEAD,
            );
        },
    )
    .modifier(
        Modifier::new()
            .width(10.0)
            .height(RULER_HEIGHT)
            .absolute()
            .offset(Some(x - 5.0), Some(0.0), None, None)
            .z_index(100.0)
            .hit_passthrough(),
    )
}