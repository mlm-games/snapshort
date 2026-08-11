//! Fixed ruler row: time ticks, markers, and the playhead head.

use super::geometry::{
    format_ruler_time, timeline_duration_us, timeline_width, window_to_us, TimelineScale,
    RULER_HEIGHT, TRACK_HEADER_WIDTH,
};
use crate::state::{Store, TimelineMarker};
use miniter_domain::Timestamp;
use repose_core::{
    input::{PointerButton, PointerEventKind},
    Modifier, PaintDesc, Vec2, VectorMeshData, VectorVertex, View,
};
use repose_ui::{scroll::ScrollStateXY, Box, Column, Row, Text, TextStyle, ViewExt};
use snapshort_ui_core::colors;
use snapshort_usecases::PlaybackCommand;
use std::rc::Rc;
use std::sync::Arc;

pub fn ruler_row(
    store: Rc<Store>,
    scroll_state_xy: Rc<ScrollStateXY>,
    scale: TimelineScale,
    panel_origin: Vec2,
) -> View {
    let timeline = store.state.timeline.get();
    let total_us = timeline_duration_us(timeline.as_ref());
    let total_w = timeline_width(timeline.as_ref(), scale);
    let scroll_x = scroll_state_xy.get().0;
    let vp_w = scroll_state_xy.viewport().0.max(200.0);

    let mut children: Vec<View> = Vec::new();

    let major_us = scale.major_tick_us();
    let minor_us = (major_us / 5).max(1);
    let px_per_minor = minor_us as f32 * scale.px_per_us;

    if px_per_minor >= 2.0 {
        let first = ((scroll_x / px_per_minor.max(0.001)).floor() as i64).max(0);
        let last = ((scroll_x + vp_w) / px_per_minor.max(0.001)).ceil() as i64;

        for i in first..=last {
            let us = i * minor_us;
            if us > total_us {
                break;
            }
            let x = us as f32 * scale.px_per_us;

            if us % major_us == 0 {
                let label = format_ruler_time(us);
                children.push(Box(Modifier::new()
                    .absolute()
                    .offset(Some(x), Some(4.0), None, None)
                    .width(1.0)
                    .height(6.0)
                    .background(colors::TEXT_MUTED)
                    .hit_passthrough()));
                children.push(
                    Box(Modifier::new()
                        .absolute()
                        .offset(Some(x + 4.0), Some(10.0), None, None)
                        .z_index(2.0)
                        .hit_passthrough())
                    .child(
                        Text(label)
                            .size(8.0)
                            .color(colors::TEXT_MUTED)
                            .single_line(),
                    ),
                );
            } else {
                children.push(Box(Modifier::new()
                    .absolute()
                    .offset(Some(x), Some(4.0), None, None)
                    .width(1.0)
                    .height(3.0)
                    .background(colors::TEXT_MUTED)
                    .hit_passthrough()));
            }
        }
    }

    for m in store.state.timeline_markers.get() {
        children.push(marker_view(store.clone(), m, scale));
    }

    children.push(playhead_head_view(store.clone(), scale));

    let store_for_seek = store.clone();
    let scroll_for_seek = scroll_state_xy.clone();
    let store_for_scroll = store.clone();
    let scroll_for_scroll = scroll_state_xy.clone();

    let ruler_content = Column(Modifier::new().width(total_w.max(1.0))).child(children);

    let content_area = Box(Modifier::new()
        .fill_max_width()
        .height(RULER_HEIGHT)
        .clip_rounded(0.0)
        .on_pointer_down(move |event| {
            // PointerEvent.position is local to this hit region; convert to
            // window space, then to timeline us via the shared geometry.
            let scroll_x = scroll_for_seek.get().0;
            let us = window_to_us(event.position_in_window().x, panel_origin, scroll_x, scale);
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
            } else if delta.x.abs() > 0.001 {
                let (x, y) = scroll_for_scroll.get();
                scroll_for_scroll.set_offset_xy(x + delta.x, y);
            }
            Vec2::default()
        }))
    .child(
        Box(Modifier::new()
            .width(total_w.max(1.0))
            .height(RULER_HEIGHT)
            .absolute()
            .offset(Some(-scroll_x), Some(0.0), None, None))
        .child(ruler_content),
    );

    Row(Modifier::new()
        .fill_max_width()
        .height(RULER_HEIGHT)
        .background(colors::BG_PANEL)
        .border(1.0, colors::BORDER, 0.0))
    .child((
        Box(Modifier::new()
            .width(TRACK_HEADER_WIDTH)
            .height(RULER_HEIGHT)
            .background(colors::BG_PANEL)
            .align_items(repose_core::AlignItems::CENTER)
            .justify_content(repose_core::AlignContent::CENTER))
        .child(
            Text("Time")
                .size(10.0)
                .color(colors::TEXT_MUTED)
                .single_line(),
        ),
        content_area,
    ))
}

fn marker_view(store: Rc<Store>, m: TimelineMarker, scale: TimelineScale) -> View {
    let x = scale.us_to_x(m.timestamp_us);
    let ts = m.timestamp_us;
    let label = m.label.clone();
    let store_for_mk = store.clone();

    // NO hit_passthrough on the interactive root
    Column(
        Modifier::new()
            .absolute()
            .offset(Some(x - 4.0), Some(0.0), None, None)
            .width(80.0)
            .height(RULER_HEIGHT)
            .z_index(10.0),
    )
    .child((
        Box(Modifier::new()
            .width(10.0)
            .height(12.0)
            .background(colors::MARKER)
            .cursor(repose_core::CursorIcon::Pointer)
            .on_pointer_down(move |event| match &event.event {
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
            })),
        Box(Modifier::new()
            .absolute()
            .offset(Some(12.0), Some(0.0), None, None)
            .hit_passthrough())
        .child(Text(&label).size(8.0).color(colors::MARKER).single_line()),
    ))
}

pub fn playhead_head_view(store: Rc<Store>, scale: TimelineScale) -> View {
    let x = scale.us_to_x(store.state.playhead.get().0);

    repose_canvas::Canvas(
        Modifier::new().fill_max_height().width(10.0),
        move |scope: &mut repose_canvas::DrawScope| {
            let height = scope.size.height;
            let width = scope.size.width;
            scope.draw_rect(
                repose_core::Rect {
                    x: width / 2.0 - 0.5,
                    y: 2.0,
                    w: 1.0,
                    h: height - 2.0,
                },
                colors::PLAYHEAD,
                0.0,
            );
            draw_triangle(
                scope,
                Vec2 {
                    x: width / 2.0,
                    y: 0.0,
                },
                Vec2 {
                    x: width / 2.0 - 5.0,
                    y: 8.0,
                },
                Vec2 {
                    x: width / 2.0 + 5.0,
                    y: 8.0,
                },
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

fn draw_triangle(
    scope: &mut repose_canvas::DrawScope,
    a: Vec2,
    b: Vec2,
    c: Vec2,
    color: repose_core::Color,
) {
    let [r, g, bl, alpha] = color.to_linear();
    let vc = [r * alpha, g * alpha, bl * alpha, alpha];
    let mesh = VectorMeshData {
        vertices: Arc::new([
            VectorVertex {
                pos: [a.x, a.y],
                color: vc,
                uv: [0.0; 2],
            },
            VectorVertex {
                pos: [b.x, b.y],
                color: vc,
                uv: [0.0; 2],
            },
            VectorVertex {
                pos: [c.x, c.y],
                color: vc,
                uv: [0.0; 2],
            },
        ]),
        indices: Arc::new([0u32, 1, 2]),
    };
    scope.draw_vector_mesh(
        Arc::new(mesh),
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        PaintDesc::Solid,
    );
}
