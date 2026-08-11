//! Clip rendering: body, previews, trim handles, indicators, drag source.

use crate::views::dnd::{as_drag_payload, ClipDragPayload, TrimPayload};
use crate::views::timeline::geometry::{
    MIN_CLIP_WIDTH, TRACK_HEADER_WIDTH, TRACK_HEIGHT, TRIM_HANDLE_WIDTH, clip_label, TimelineScale,
};
use crate::state::Store;
use miniter_domain::{Clip, ClipId, ClipKind, TrackId, TrackKind};
use miniter_usecases::EditCommand;
use repose_core::{
    dnd::{DragPayload, DragStart},
    input::{PointerButton, PointerEventKind},
    CursorIcon, Modifier, Vec2, View,
};
use repose_material::Icon as IconView;
use repose_ui::{
    Box, Column, Image, ImageExt, Row, Text, TextStyle, ViewExt,
    scroll::ScrollStateXY,
};
use snapshort_ui_core::{audio_waveform, colors, Icons};
use snapshort_usecases::PreviewCommand;
use std::rc::Rc;

fn kind_icon(kind: TrackKind) -> repose_material::Symbol {
    match kind {
        TrackKind::Video => Icons::movie,
        TrackKind::Audio => Icons::music_note,
        TrackKind::Text => Icons::text_fields,
        TrackKind::Subtitle => Icons::subtitle,
        _ => Icons::movie,
    }
}

fn clip_height() -> f32 {
    TRACK_HEIGHT - 6.0
}

#[allow(clippy::too_many_arguments)]
pub fn clip_view(
    store: Rc<Store>,
    clip: &Clip,
    kind: TrackKind,
    scale: TimelineScale,
    scroll_state_xy: Rc<ScrollStateXY>,
    panel_origin: Vec2,
    selected_clip: Option<ClipId>,
    track_id: TrackId,
    locked: bool,
) -> View {
    let dur_us = clip.timeline_duration.as_micros().max(1);
    let render_w = (dur_us as f32 * scale.px_per_us).max(MIN_CLIP_WIDTH);
    let x = scale.us_to_x(clip.timeline_start.0);

    let is_selected = selected_clip == Some(clip.id);
    let clip_id = clip.id;
    let original_start = clip.timeline_start;
    let original_track = track_id;
    let clip_h = clip_height();
    let show_details = render_w >= 64.0;

    let (bg, border) = if is_selected {
        (colors::BG_SELECTED, colors::ACCENT_CYAN)
    } else {
        (colors::BG_TRACK, colors::TEXT_MUTED)
    };

    let store_for_click = store.clone();
    let store_for_menu = store.clone();
    let store_for_thumb = store.clone();
    let store_for_drag = store.clone();
    let scroll_for_drag = scroll_state_xy.clone();

    let body = Box(Modifier::new()
        .width(render_w)
        .height(clip_h)
        .background(bg)
        .border(if is_selected { 2.0 } else { 1.0 }, border, 4.0)
        .clip_rounded(4.0)
        .cursor(if locked { CursorIcon::Default } else { CursorIcon::Grab })
        .on_drag_start(move |event: DragStart| -> Option<DragPayload> {
            if locked {
                return None;
            }
            // DragStart.position is in window space; the clip's leading edge in
            // window space is panel_origin.x + TRACK_HEADER_WIDTH + x - scroll_x.
            let (scroll_x, _) = scroll_for_drag.get();
            let clip_window_x =
                panel_origin.x + TRACK_HEADER_WIDTH + x - scroll_x;
            let grab_offset_us =
                scale.x_to_us((event.position.x - clip_window_x).max(0.0));
            Some(as_drag_payload(ClipDragPayload {
                clip_id,
                original_start,
                original_track,
                grab_offset_us,
            }))
        })
        .on_drag_end(move |_| {
            store_for_drag.state.timeline_snap_indicator.set(None);
        }))
    .child(Box(Modifier::new().padding(4.0))
        .child(Column(Modifier::new().fill_max_width()).child((
            Row(Modifier::new().fill_max_width()).child((
                Text(clip_label(clip)).size(10.0).color(colors::TEXT_PRIMARY).single_line(),
                Box(Modifier::new().flex_grow(1.0)),
                speed_badge(clip),
                mute_icon(clip),
            )),
            if show_details {
                preview_content(store_for_thumb, clip, kind, render_w, clip_h)
            } else {
                Box(Modifier::new().width(1.0).height(1.0))
            },
        ))));

    let left_handle = Box(Modifier::new()
        .width(TRIM_HANDLE_WIDTH)
        .height(clip_h)
        .background(if is_selected { colors::ACCENT_CYAN } else { colors::TRANSPARENT })
        .cursor(if locked { CursorIcon::Default } else { CursorIcon::EwResize })
        .on_drag_start(move |_: DragStart| -> Option<DragPayload> {
            if locked {
                return None;
            }
            Some(as_drag_payload(TrimPayload {
                clip_id,
                is_start: true,
            }))
        })
        .on_drag_end(move |_| {}));

    let right_handle = Box(Modifier::new()
        .width(TRIM_HANDLE_WIDTH)
        .height(clip_h)
        .background(if is_selected { colors::ACCENT_CYAN } else { colors::TRANSPARENT })
        .cursor(if locked { CursorIcon::Default } else { CursorIcon::EwResize })
        .on_drag_start(move |_: DragStart| -> Option<DragPayload> {
            if locked {
                return None;
            }
            Some(as_drag_payload(TrimPayload {
                clip_id,
                is_start: false,
            }))
        })
        .on_drag_end(move |_| {}));

    let mut stack_children: Vec<View> = vec![
        body,
        left_handle
            .modifier(Modifier::new()
                .absolute()
                .offset(Some(0.0), Some(0.0), None, None)
                .z_index(10.0)),
        right_handle
            .modifier(Modifier::new()
                .absolute()
                .offset(Some(render_w - TRIM_HANDLE_WIDTH), Some(0.0), None, None)
                .z_index(10.0)),
    ];

    if clip.transition_in.is_some() {
        stack_children.push(transition_indicator(render_w, clip_h, true));
    }
    if clip.transition_out.is_some() {
        stack_children.push(transition_indicator(render_w, clip_h, false));
    }

    if !clip.keyframes.keyframes.is_empty() {
        stack_children.push(keyframe_indicator(render_w));
    }

    stack_children.push(
        Box(Modifier::new()
            .absolute()
            .offset(Some(2.0), Some(clip_h - 14.0), None, None)
            .width(14.0)
            .height(12.0)
            .z_index(11.0)
            .hit_passthrough())
        .child(IconView(kind_icon(kind)).size(11.0).color(colors::TEXT_MUTED)),
    );

    let view = Column(Modifier::new()
        .size(render_w, clip_h)
        .on_pointer_down(move |event| {
            store_for_click.state.selected_clip_id.set(Some(clip_id));
            store_for_click.state.selected_asset_id.set(None);
            if matches!(&event.event, PointerEventKind::Down(PointerButton::Secondary)) {
                let window_pos = event.position_in_window();
                store_for_menu.state.selected_clip_id.set(Some(clip_id));
                store_for_menu.open_clip_menu(window_pos, clip_id, track_id);
            }
        })
        .on_action({
            let store = store.clone();
            move |action| {
                if let repose_core::shortcuts::Action::Custom(name) = action {
                    if name.as_ref() == "timeline:delete" {
                        store.dispatch_edit(EditCommand::RemoveClip { clip_id });
                        store.state.selected_clip_id.set(None);
                        return true;
                    }
                }
                false
            }
        }))
    .child(stack_children);

    Box(Modifier::new()
        .width(render_w)
        .height(clip_h)
        .absolute()
        .offset(Some(x), Some(3.0), None, None)
        .z_index(2.0))
    .child(view)
}

fn speed_badge(clip: &Clip) -> View {
    let speed = clip.speed;
    if (speed - 1.0).abs() < 0.001 {
        return Box(Modifier::new().width(1.0).height(1.0));
    }
    Box(Modifier::new()
        .padding_values(repose_core::PaddingValues { left: 3.0, right: 3.0, top: 0.0, bottom: 0.0 })
        .background(colors::TEXT_ACCENT)
        .clip_rounded(3.0))
    .child(
        Text(format!("{:.1}×", speed)).size(8.0).color(colors::BG_DARK).single_line(),
    )
}

fn mute_icon(clip: &Clip) -> View {
    if !clip.muted {
        return Box(Modifier::new().width(1.0).height(1.0));
    }
    Box(Modifier::new()
        .padding_values(repose_core::PaddingValues { left: 3.0, right: 3.0, top: 0.0, bottom: 0.0 }))
    .child(IconView(Icons::volume_off).size(10.0).color(colors::WARNING))
}

fn preview_content(
    store: Rc<Store>,
    clip: &Clip,
    kind: TrackKind,
    render_w: f32,
    clip_h: f32,
) -> View {
    if kind == TrackKind::Audio {
        let waveform_width = (render_w - 24.0).max(10.0);
        let waveform_height = (clip_h - 22.0).clamp(8.0, 18.0);
        let wave_data: Vec<f32> = match &clip.kind {
            ClipKind::Audio(a) => {
                let assets = store.state.assets.get();
                let sp = a.source_path.as_str();
                assets
                    .iter()
                    .find(|a| a.effective_path().to_string_lossy().as_ref() == sp)
                    .and_then(|a| a.media_info.as_ref())
                    .and_then(|m| m.waveform.clone())
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        };
        return audio_waveform(
            waveform_width,
            waveform_height,
            if wave_data.is_empty() { None } else { Some(wave_data.as_slice()) },
            colors::AUDIO_TRACK,
        );
    }

    clip_thumbnails(store, clip, render_w, clip_h)
}

fn clip_thumbnails(store: Rc<Store>, clip: &Clip, width: f32, clip_h: f32) -> View {
    let Some(asset_id) = (|| {
        let source_path = match &clip.kind {
            ClipKind::Video(v) => v.source_path.as_str(),
            _ => return None,
        };
        let assets = store.state.assets.get();
        let asset = assets
            .iter()
            .find(|a| a.effective_path().to_string_lossy().as_ref() == source_path)?;
        Some(asset.id)
    })() else {
        return Box(Modifier::new().width(width).height(1.0));
    };

    let num_thumbnails = ((width / 100.0).ceil() as usize).clamp(2, 20);
    let timeline_dur_us = clip.timeline_duration.as_micros().max(1);
    let source_start_us = clip.source_start.as_micros();
    let source_dur_us = (clip.source_end.as_micros() - source_start_us).max(1);
    let speed = clip.speed.max(0.01);
    let thumb_height = (clip_h - 22.0).max(8.0);

    let mut handles: Vec<(f32, repose_core::ImageHandle)> = Vec::new();
    for i in 0..num_thumbnails {
        let t = (i as f64 + 0.5) / num_thumbnails as f64;
        // Walk source by timeline progress scaled through playback speed, so
        // sped-up / trimmed clips sample the correct source region.
        let source_time = (source_start_us as f64 + t * timeline_dur_us as f64 * speed)
            .min(source_start_us as f64 + source_dur_us as f64)
            .max(source_start_us as f64) as i64;
        let key = (asset_id, source_time);

        let cached = store
            .timeline_thumb_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&key).copied());

        if let Some(handle) = cached {
            handles.push((1.0 / num_thumbnails as f32, handle));
        } else {
            store.dispatch_preview(PreviewCommand::RequestTimelineThumbnail {
                asset_id,
                source_time,
            });
        }
    }

    if handles.is_empty() {
        return Box(Modifier::new().width(width).height(thumb_height));
    }

    let children: Vec<View> = handles
        .into_iter()
        .map(|(frac, handle)| {
            Image(
                Modifier::new()
                    .width(width * frac)
                    .height(thumb_height),
                handle,
            )
            .image_fit(repose_core::ImageFit::Cover)
        })
        .collect();

    Row(Modifier::new().width(width).height(thumb_height)).child(children)
}

fn transition_indicator(render_w: f32, clip_h: f32, is_in: bool) -> View {
    let x = if is_in { 0.0 } else { render_w - 4.0 };
    Box(Modifier::new()
        .width(4.0)
        .height(clip_h * 0.5)
        .background(colors::ACCENT)
        .absolute()
        .offset(Some(x), Some(clip_h * 0.25), None, None)
        .z_index(12.0))
}

fn keyframe_indicator(render_w: f32) -> View {
    Box(Modifier::new()
        .size(6.0, 6.0)
        .background(colors::WARNING)
        .border(1.0, colors::BG_DARK, 1.0)
        .absolute()
        .offset(Some(render_w / 2.0 - 3.0), Some(2.0), None, None)
        .z_index(13.0))
}