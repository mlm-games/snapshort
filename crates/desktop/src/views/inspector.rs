use crate::state::Store;
use miniter_domain::{
    filter::AudioFilter, filter::VideoEffect, filter::VideoFilter, AudioClip, BlendMode, Clip,
    ClipId, ClipKind, Easing, Keyframe, KeyframeCurve, MaskComposition, MaskEffect, MaskOperation,
    MaskShape, MaskSource, MaskTransform, MediaDuration, SubtitleClip, TextAlignment, TextOverlay,
    TextStyle, Timestamp, Track, Transition, TransitionKind, VideoClip,
};
use miniter_usecases::EditCommand;
use repose_core::{
    prelude::theme, AlignItems, Color, Modifier, PaddingValues, View,
};
use repose_material::{material3, Icon};
use repose_ui::{
    scroll::{remember_scroll_state, ScrollArea},
    textfield::{get_textfield_state, set_textfield_state},
    BasicTextField, Box, Column, Row, Text, TextFieldConfig, TextFieldState,
    TextStyle as ReposeTextStyle, ViewExt,
};
use snapshort_ui_core::Icons;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

thread_local! {
    static UI_FLAGS: RefCell<HashMap<String, bool>> = RefCell::new(HashMap::new());
    static UI_STRING_VALS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

fn flag_get(key: &str) -> bool {
    UI_FLAGS.with(|m| m.borrow().get(key).copied().unwrap_or(false))
}

fn flag_set(key: &str, val: bool) {
    UI_FLAGS.with(|m| m.borrow_mut().insert(key.to_owned(), val));
}

fn flag_toggle(key: &str) {
    let v = flag_get(key);
    flag_set(key, !v);
}

fn str_val_get(key: &str) -> Option<String> {
    UI_STRING_VALS
        .with(|m| m.borrow().get(key).cloned())
}

fn str_val_set(key: &str, val: String) {
    UI_STRING_VALS.with(|m| m.borrow_mut().insert(key.to_owned(), val));
}

fn h_spacer(w: f32) -> View {
    Box(Modifier::new().width(w))
}

fn v_spacer(h: f32) -> View {
    Box(Modifier::new().height(h))
}

fn kv(label: impl Into<String>, value: impl Into<String>) -> View {
    let th = theme();
    Row(Modifier::new()
        .fill_max_width()
        .height(22.0)
        .align_items(AlignItems::CENTER))
    .child(vec![
        Text(label.into()).size(11.0).color(th.on_surface_variant),
        Box(Modifier::new().flex_grow(1.0)),
        Text(value.into()).size(11.0).color(th.on_surface),
    ])
}

fn section_header(label: &str) -> View {
    let th = theme();
    Column(Modifier::new().fill_max_width()).child((
        v_spacer(8.0),
        Box(Modifier::new()
            .fill_max_width()
            .height(1.0)
            .background(th.outline.with_alpha(80))),
        v_spacer(6.0),
        Text(label).size(11.0).color(th.primary),
        v_spacer(4.0),
    ))
}

fn slider_row(
    label: &str,
    value: f32,
    min: f32,
    max: f32,
    step: Option<f32>,
    display: impl Fn(f32) -> String + 'static,
    on_change: impl Fn(f32) + 'static,
) -> View {
    let th = theme();
    let display_val = display(value);
    Column(Modifier::new().fill_max_width().padding_values(PaddingValues {
        left: 0.0,
        right: 0.0,
        top: 2.0,
        bottom: 2.0,
    }))
    .child((
        Row(Modifier::new()
            .fill_max_width()
            .align_items(AlignItems::CENTER))
        .child((
            Text(label).size(11.0).color(th.on_surface_variant),
            Box(Modifier::new().flex_grow(1.0)),
            Text(display_val).size(10.0).color(th.on_surface),
        )),
        material3::Slider(value, (min, max), step, on_change, Default::default())
            .modifier(Modifier::new().fill_max_width().height(20.0)),
    ))
}

fn slider_row_with_kf(
    label: &str,
    value: f32,
    min: f32,
    max: f32,
    step: Option<f32>,
    display: impl Fn(f32) -> String + 'static,
    on_change: impl Fn(f32) + 'static,
    has_kf: bool,
    on_kf_toggle: impl Fn() + 'static,
) -> View {
    let th = theme();
    let display_val = display(value);
    Column(Modifier::new().fill_max_width().padding_values(PaddingValues {
        left: 0.0,
        right: 0.0,
        top: 2.0,
        bottom: 2.0,
    }))
    .child((
        Row(Modifier::new()
            .fill_max_width()
            .align_items(AlignItems::CENTER))
        .child((
            Text(label).size(11.0).color(th.on_surface_variant),
            Box(Modifier::new().flex_grow(1.0)),
            material3::IconButton(
                Icon(Icons::diamond)
                    .size(12.0)
                    .color(if has_kf { th.primary } else { th.on_surface_variant }),
                on_kf_toggle,
                Default::default(),
            ),
            h_spacer(4.0),
            Text(display_val).size(10.0).color(th.on_surface),
        )),
        material3::Slider(value, (min, max), step, on_change, Default::default())
            .modifier(Modifier::new().fill_max_width().height(20.0)),
    ))
}

fn fmt_us(us: i64) -> String {
    let abs = us.unsigned_abs();
    let millis = abs / 1000;
    let secs = millis / 1000;
    let mins = secs / 60;
    let hours = mins / 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}.{:03}", hours, mins % 60, secs % 60, millis % 1000)
    } else if mins > 0 {
        format!("{:02}:{:02}.{:03}", mins, secs % 60, millis % 1000)
    } else {
        format!("{}.{:03}s", secs, millis % 1000)
    }
}

fn fmt_pct(v: f32) -> String {
    format!("{}%", (v * 100.0).round() as i32)
}

fn clip_name(clip: &Clip) -> String {
    match &clip.kind {
        ClipKind::Video(v) => {
            v.source_path
                .split('/')
                .last()
                .or_else(|| v.source_path.split('\\').last())
                .unwrap_or("Video")
                .to_string()
        }
        ClipKind::Audio(a) => {
            a.source_path
                .split('/')
                .last()
                .or_else(|| a.source_path.split('\\').last())
                .unwrap_or("Audio")
                .to_string()
        }
        ClipKind::Text(t) => {
            if t.text.len() > 30 {
                format!("{}…", &t.text[..30])
            } else {
                t.text.clone()
            }
        }
        ClipKind::Subtitle(s) => {
            s.source_path
                .split('/')
                .last()
                .or_else(|| s.source_path.split('\\').last())
                .unwrap_or("Subtitle")
                .to_string()
        }
        _ => "Clip".to_string(),
    }
}

fn filter_label(filter: &VideoFilter) -> &'static str {
    match filter {
        VideoFilter::Brightness { .. } => "Brightness",
        VideoFilter::Contrast { .. } => "Contrast",
        VideoFilter::Saturation { .. } => "Saturation",
        VideoFilter::Grayscale => "Grayscale",
        VideoFilter::Blur { .. } => "Blur",
        VideoFilter::Sharpen { .. } => "Sharpen",
        VideoFilter::Sepia => "Sepia",
        VideoFilter::Hue { .. } => "Hue",
        VideoFilter::Crop { .. } => "Crop",
        VideoFilter::Rotate { .. } => "Rotate",
        VideoFilter::Flip { .. } => "Flip",
        VideoFilter::Transform { .. } => "Transform",
        VideoFilter::Speed { .. } => "Speed",
        VideoFilter::Opacity { .. } => "Opacity",
        VideoFilter::BlendMode { .. } => "Blend",
        _ => "Unknown",
    }
}

struct ParamSliderDef {
    label: &'static str,
    value: f32,
    min: f32,
    max: f32,
    step: Option<f32>,
}

fn filter_param_sliders(filter: &VideoFilter) -> Vec<ParamSliderDef> {
    match filter {
        VideoFilter::Brightness { value } => vec![ParamSliderDef {
            label: "Value",
            value: *value,
            min: -1.0,
            max: 1.0,
            step: Some(0.01),
        }],
        VideoFilter::Contrast { value } => vec![ParamSliderDef {
            label: "Value",
            value: *value,
            min: 0.0,
            max: 2.0,
            step: Some(0.01),
        }],
        VideoFilter::Saturation { value } => vec![ParamSliderDef {
            label: "Value",
            value: *value,
            min: 0.0,
            max: 2.0,
            step: Some(0.01),
        }],
        VideoFilter::Grayscale => vec![],
        VideoFilter::Blur { radius } => vec![ParamSliderDef {
            label: "Radius",
            value: *radius,
            min: 0.0,
            max: 50.0,
            step: Some(0.5),
        }],
        VideoFilter::Sharpen { amount } => vec![ParamSliderDef {
            label: "Amount",
            value: *amount,
            min: 0.0,
            max: 5.0,
            step: Some(0.1),
        }],
        VideoFilter::Sepia => vec![],
        VideoFilter::Hue { degrees } => vec![ParamSliderDef {
            label: "Degrees",
            value: *degrees,
            min: 0.0,
            max: 360.0,
            step: Some(1.0),
        }],
        VideoFilter::Crop { left, top, right, bottom } => {
            vec![
                ParamSliderDef {
                    label: "Left",
                    value: *left,
                    min: 0.0,
                    max: 1.0,
                    step: Some(0.01),
                },
                ParamSliderDef {
                    label: "Top",
                    value: *top,
                    min: 0.0,
                    max: 1.0,
                    step: Some(0.01),
                },
                ParamSliderDef {
                    label: "Right",
                    value: *right,
                    min: 0.0,
                    max: 1.0,
                    step: Some(0.01),
                },
                ParamSliderDef {
                    label: "Bottom",
                    value: *bottom,
                    min: 0.0,
                    max: 1.0,
                    step: Some(0.01),
                },
            ]
        }
        VideoFilter::Rotate { degrees } => vec![ParamSliderDef {
            label: "Degrees",
            value: *degrees,
            min: -180.0,
            max: 180.0,
            step: Some(1.0),
        }],
        VideoFilter::Flip { .. } => vec![],
        VideoFilter::Transform { scale, translate_x, translate_y, rotate } => {
            vec![
                ParamSliderDef {
                    label: "Scale",
                    value: *scale,
                    min: 0.0,
                    max: 4.0,
                    step: Some(0.01),
                },
                ParamSliderDef {
                    label: "Pan X",
                    value: *translate_x,
                    min: -2000.0,
                    max: 2000.0,
                    step: Some(1.0),
                },
                ParamSliderDef {
                    label: "Pan Y",
                    value: *translate_y,
                    min: -2000.0,
                    max: 2000.0,
                    step: Some(1.0),
                },
                ParamSliderDef {
                    label: "Rotate",
                    value: *rotate,
                    min: -180.0,
                    max: 180.0,
                    step: Some(0.5),
                },
            ]
        }
        VideoFilter::Speed { factor } => vec![ParamSliderDef {
            label: "Factor",
            value: *factor as f32,
            min: 0.1,
            max: 10.0,
            step: Some(0.1),
        }],
        VideoFilter::Opacity { value } => vec![ParamSliderDef {
            label: "Value",
            value: *value,
            min: 0.0,
            max: 1.0,
            step: Some(0.01),
        }],
        VideoFilter::BlendMode { .. } => vec![],
        _ => vec![],
    }
}

fn filter_apply_value(filter: &VideoFilter, idx: usize, new_val: f32) -> VideoFilter {
    match filter {
        VideoFilter::Brightness { .. } => VideoFilter::Brightness { value: new_val },
        VideoFilter::Contrast { .. } => VideoFilter::Contrast { value: new_val },
        VideoFilter::Saturation { .. } => VideoFilter::Saturation { value: new_val },
        VideoFilter::Grayscale => VideoFilter::Grayscale,
        VideoFilter::Blur { .. } => VideoFilter::Blur { radius: new_val },
        VideoFilter::Sharpen { .. } => VideoFilter::Sharpen { amount: new_val },
        VideoFilter::Sepia => VideoFilter::Sepia,
        VideoFilter::Hue { .. } => VideoFilter::Hue { degrees: new_val },
        VideoFilter::Crop { left, top, right, bottom } => {
            match idx {
                0 => VideoFilter::Crop { left: new_val, top: *top, right: *right, bottom: *bottom },
                1 => VideoFilter::Crop { left: *left, top: new_val, right: *right, bottom: *bottom },
                2 => VideoFilter::Crop { left: *left, top: *top, right: new_val, bottom: *bottom },
                _ => VideoFilter::Crop { left: *left, top: *top, right: *right, bottom: new_val },
            }
        }
        VideoFilter::Rotate { .. } => VideoFilter::Rotate { degrees: new_val },
        VideoFilter::Flip { horizontal, vertical } => VideoFilter::Flip { horizontal: *horizontal, vertical: *vertical },
        VideoFilter::Transform { scale, translate_x, translate_y, rotate } => {
            match idx {
                0 => VideoFilter::Transform { scale: new_val, translate_x: *translate_x, translate_y: *translate_y, rotate: *rotate },
                1 => VideoFilter::Transform { scale: *scale, translate_x: new_val, translate_y: *translate_y, rotate: *rotate },
                2 => VideoFilter::Transform { scale: *scale, translate_x: *translate_x, translate_y: new_val, rotate: *rotate },
                _ => VideoFilter::Transform { scale: *scale, translate_x: *translate_x, translate_y: *translate_y, rotate: new_val },
            }
        }
        VideoFilter::Speed { .. } => VideoFilter::Speed { factor: new_val as f64 },
        VideoFilter::Opacity { .. } => VideoFilter::Opacity { value: new_val },
        VideoFilter::BlendMode { .. } => VideoFilter::BlendMode { mode: BlendMode::Normal },
        _ => filter.clone(),
    }
}

fn audio_filter_label(filter: &AudioFilter) -> &'static str {
    match filter {
        AudioFilter::Volume { .. } => "Volume",
        AudioFilter::FadeIn { .. } => "Fade In",
        AudioFilter::FadeOut { .. } => "Fade Out",
        AudioFilter::Normalize => "Normalize",
        _ => "Unknown",
    }
}

fn audio_filter_duration_us(filter: &AudioFilter) -> Option<i64> {
    match filter {
        AudioFilter::FadeIn { duration_us } => Some(*duration_us),
        AudioFilter::FadeOut { duration_us } => Some(*duration_us),
        _ => None,
    }
}

fn clip_has_keyframes_for(clip: &Clip, param: &str) -> bool {
    clip.keyframes.keyframes.iter().any(|k| k.param == param)
}

pub fn inspector_panel(store: Rc<Store>) -> View {
    let selected_clip_id = store.state.selected_clip_id.get();
    let selected_asset_id = store.state.selected_asset_id.get();
    let timeline = store.state.timeline.get();
    let assets = store.state.assets.get();

    if let (Some(clip_id), Some(tl)) = (selected_clip_id, timeline) {
        let clip_track = tl.tracks.iter().find_map(|t| {
            t.clip_by_id(clip_id).map(|c| (c.clone(), t.clone()))
        });
        if let Some((clip, track)) = clip_track {
            return clip_inspector(store, &clip, &track);
        }
    }

    if let Some(asset_id) = selected_asset_id {
        if let Some(a) = assets.iter().find(|a| a.id == asset_id) {
            return asset_inspector(a);
        }
    }

    empty_inspector()
}

fn empty_inspector() -> View {
    let th = theme();
    ScrollArea(
        Modifier::new().fill_max_size(),
        remember_scroll_state("inspector_empty"),
        Column(Modifier::new().fill_max_width().padding(10.0)).child((
            Text("Inspector").size(12.0).color(th.on_surface_variant),
            v_spacer(6.0),
            Text("Select a clip or asset to edit its properties.")
                .size(11.0)
                .color(th.on_surface_variant.with_alpha(160)),
        )),
    )
}

fn asset_inspector(asset: &snapshort_usecases::Asset) -> View {
    let th = theme();
    let path = asset.path.to_string_lossy().to_string();
    let status = format!("{:?}", asset.status);
    let dur_us = asset
        .media_info
        .as_ref()
        .map(|m| m.duration_ms as i64 * 1000)
        .unwrap_or(0);

    ScrollArea(
        Modifier::new().fill_max_size(),
        remember_scroll_state("inspector_asset"),
        Column(Modifier::new().fill_max_width().padding(10.0)).child((
            Text("Asset").size(12.0).color(th.on_surface),
            v_spacer(8.0),
            kv("Name", asset.name.clone()),
            kv("Type", format!("{:?}", asset.asset_type)),
            kv("Status", status),
            kv("Duration", fmt_us(dur_us)),
            kv("Path", path),
        )),
    )
}

fn clip_inspector(store: Rc<Store>, clip: &Clip, track: &Track) -> View {
    let th = theme();
    let track_label = match track.kind {
        miniter_domain::TrackKind::Video => "V",
        miniter_domain::TrackKind::Audio => "A",
        miniter_domain::TrackKind::Text => "T",
        miniter_domain::TrackKind::Subtitle => "S",
        _ => "?",
    };

    let name = clip_name(clip);
    let start_fmt = fmt_us(clip.timeline_start.0);
    let end_fmt = fmt_us(clip.timeline_end().0);
    let dur_fmt = fmt_us(clip.timeline_duration.0);

    let mut children: Vec<View> = Vec::new();

    children.push(Text(&name).size(12.0).color(th.on_surface));
    children.push(v_spacer(8.0));
    children.push(kv("Clip ID", &clip.id.0.to_string()[..8]));
    children.push(kv("Track", track_label));
    children.push(kv("Start", &start_fmt));
    children.push(kv("End", &end_fmt));
    children.push(kv("Duration", &dur_fmt));

    match &clip.kind {
        ClipKind::Video(v) => video_clip_properties(store, clip, v, &mut children),
        ClipKind::Audio(a) => audio_clip_properties(store, clip, a, &mut children),
        ClipKind::Text(t) => text_clip_properties(store, clip, t, &mut children),
        ClipKind::Subtitle(s) => subtitle_clip_properties(store, clip, s, &mut children),
        _ => {}
    }

    children.push(v_spacer(16.0));

    ScrollArea(
        Modifier::new().fill_max_size(),
        remember_scroll_state("inspector_clip"),
        Column(Modifier::new().fill_max_width().padding(10.0)).child(children),
    )
}

fn video_clip_properties(store: Rc<Store>, clip: &Clip, v: &VideoClip, children: &mut Vec<View>) {
    let clip_id = clip.id;

    children.push(section_header("Video"));
    children.push(kv("Source", &v.source_path));
    children.push(kv("Resolution", format!("{}x{}", v.width, v.height)));
    children.push(kv("FPS", format!("{:.1}", v.fps)));

    children.push(section_header("Properties"));

    let has_kf_speed = clip_has_keyframes_for(clip, "speed");
    let speed_store = store.clone();
    let speed_cid = clip_id;
    children.push(slider_row_with_kf(
        "Speed",
        clip.speed as f32,
        0.1, 4.0, Some(0.1),
        |v| format!("{:.1}x", v),
        {
            let store = store.clone();
            let cid = clip_id;
            move |v| store.dispatch_edit(EditCommand::SetClipSpeed {
                clip_id: cid,
                speed: v as f64,
            })
        },
        has_kf_speed,
        {
            let store = speed_store;
            let cid = speed_cid;
            move || {
                let playhead = store.state.playhead.get();
                store.dispatch_edit(EditCommand::AddKeyframe {
                    clip_id: cid,
                    keyframe: Keyframe {
                        param: "speed".into(),
                        offset: MediaDuration(playhead.0),
                        value: store.state.timeline.get().and_then(|tl| {
                            tl.tracks.iter().find_map(|t| t.clip_by_id(cid).map(|c| c.speed as f32))
                        }).unwrap_or(1.0),
                        easing: Easing::Linear,
                    },
                })
            }
        },
    ));

    let has_kf_vol = clip_has_keyframes_for(clip, "volume");
    let vol_store = store.clone();
    let vol_cid = clip_id;
    children.push(slider_row_with_kf(
        "Volume",
        clip.volume,
        0.0, 2.0, None,
        |v| fmt_pct(v / 2.0),
        {
            let store = store.clone();
            let cid = clip_id;
            move |v| store.dispatch_edit(EditCommand::SetClipVolume {
                clip_id: cid,
                volume: v,
            })
        },
        has_kf_vol,
        {
            let store = vol_store;
            let cid = vol_cid;
            move || {
                let playhead = store.state.playhead.get();
                store.dispatch_edit(EditCommand::AddKeyframe {
                    clip_id: cid,
                    keyframe: Keyframe {
                        param: "volume".into(),
                        offset: MediaDuration(playhead.0),
                        value: store.state.timeline.get().and_then(|tl| {
                            tl.tracks.iter().find_map(|t| t.clip_by_id(cid).map(|c| c.volume))
                        }).unwrap_or(1.0),
                        easing: Easing::Linear,
                    },
                })
            }
        },
    ));

    let has_kf_op = clip_has_keyframes_for(clip, "opacity");
    let op_store = store.clone();
    let op_cid = clip_id;
    children.push(slider_row_with_kf(
        "Opacity",
        clip.opacity,
        0.0, 1.0, None,
        fmt_pct,
        {
            let store = store.clone();
            let cid = clip_id;
            move |v| store.dispatch_edit(EditCommand::SetClipOpacity {
                clip_id: cid,
                opacity: v,
            })
        },
        has_kf_op,
        {
            let store = op_store;
            let cid = op_cid;
            move || {
                let playhead = store.state.playhead.get();
                store.dispatch_edit(EditCommand::AddKeyframe {
                    clip_id: cid,
                    keyframe: Keyframe {
                        param: "opacity".into(),
                        offset: MediaDuration(playhead.0),
                        value: store.state.timeline.get().and_then(|tl| {
                            tl.tracks.iter().find_map(|t| t.clip_by_id(cid).map(|c| c.opacity))
                        }).unwrap_or(1.0),
                        easing: Easing::Linear,
                    },
                })
            }
        },
    ));

    children.push(section_header("Transitions"));
    children.push(transition_selector(
        store.clone(),
        clip_id,
        clip.transition_in.as_ref(),
        true,
    ));
    children.push(transition_selector(
        store.clone(),
        clip_id,
        clip.transition_out.as_ref(),
        false,
    ));

    children.push(section_header("Video Filters"));
    if v.filters.is_empty() {
        let th = theme();
        children.push(
            Text("No filters").size(10.0).color(th.on_surface_variant.with_alpha(160)),
        );
    }
    for (idx, effect) in v.filters.iter().enumerate() {
        filter_row(store.clone(), clip_id, idx, effect, children);
    }
    add_filter_dropdown(store.clone(), clip_id, children);

    if !v.masks.is_empty() {
        mask_section(store.clone(), clip_id, &v.masks, children);
    }

    if !v.audio_filters.is_empty() {
        children.push(section_header("Audio Filters"));
        for (idx, af) in v.audio_filters.iter().enumerate() {
            audio_filter_row(store.clone(), clip_id, idx, af, true, children);
        }
    }

    keyframe_section(store.clone(), clip, children);
}

fn audio_clip_properties(store: Rc<Store>, clip: &Clip, a: &AudioClip, children: &mut Vec<View>) {
    let clip_id = clip.id;

    children.push(section_header("Audio"));
    children.push(kv("Source", &a.source_path));

    children.push(section_header("Properties"));

    let has_kf = clip_has_keyframes_for(clip, "volume");
    let kf_store = store.clone();
    let kf_cid = clip_id;
    children.push(slider_row_with_kf(
        "Volume",
        clip.volume,
        0.0, 2.0, None,
        |v| fmt_pct(v / 2.0),
        {
            let store = store.clone();
            let cid = clip_id;
            move |v| store.dispatch_edit(EditCommand::SetClipVolume {
                clip_id: cid,
                volume: v,
            })
        },
        has_kf,
        {
            let store = kf_store;
            let cid = kf_cid;
            move || {
                let playhead = store.state.playhead.get();
                store.dispatch_edit(EditCommand::AddKeyframe {
                    clip_id: cid,
                    keyframe: Keyframe {
                        param: "volume".into(),
                        offset: MediaDuration(playhead.0),
                        value: store.state.timeline.get().and_then(|tl| {
                            tl.tracks.iter().find_map(|t| t.clip_by_id(cid).map(|c| c.volume))
                        }).unwrap_or(1.0),
                        easing: Easing::Linear,
                    },
                })
            }
        },
    ));

    children.push(section_header("Audio Filters"));
    if a.filters.is_empty() {
        let th = theme();
        children.push(
            Text("No filters").size(10.0).color(th.on_surface_variant.with_alpha(160)),
        );
    }
    for (idx, filter) in a.filters.iter().enumerate() {
        audio_filter_row(store.clone(), clip_id, idx, filter, false, children);
    }

    keyframe_section(store.clone(), clip, children);
}

fn text_clip_properties(store: Rc<Store>, clip: &Clip, t: &TextOverlay, children: &mut Vec<View>) {
    let th = theme();
    let clip_id = clip.id;

    children.push(section_header("Text"));

    const TEXT_KEY: u64 = 0x54455854u64;
    let text_state = get_textfield_state(TEXT_KEY).unwrap_or_else(|| {
        let s = Rc::new(RefCell::new(TextFieldState::new()));
        s.borrow_mut().text = t.text.clone();
        set_textfield_state(TEXT_KEY, s.clone());
        s
    });

    children.push(Column(Modifier::new().fill_max_width()).child((
        Text("Content").size(11.0).color(th.on_surface_variant),
        v_spacer(4.0),
        BasicTextField(
            text_state.clone(),
            Modifier::new()
                .fill_max_width()
                .height(60.0)
                .background(th.surface_variant.with_alpha(80))
                .border(1.0, th.outline, 6.0)
                .padding(6.0),
            "Enter text…",
            TextFieldConfig {
                line_limits: repose_core::TextFieldLineLimits::MultiLine {
                    min_height_in_lines: 2,
                    max_height_in_lines: 10,
                },
                on_change: Some(Rc::new({
                    let store = store.clone();
                    let cid = clip_id;
                    move |v| {
                        store.dispatch_edit(EditCommand::UpdateTextContent {
                            clip_id: cid,
                            text: v,
                        });
                    }
                })),
                ..Default::default()
            },
        ),
    )));
    children.push(v_spacer(8.0));

    children.push(kv("Font", &t.style.font_family));
    children.push(kv("Size", format!("{:.0}px", t.style.font_size)));

    text_position_grid(store.clone(), clip_id, &t.style, children);

    let has_kf_x = clip_has_keyframes_for(clip, "text.position_x");
    let has_kf_y = clip_has_keyframes_for(clip, "text.position_y");
    text_position_sliders(store.clone(), clip, &t.style, has_kf_x, has_kf_y, children);

    let has_kf_size = clip_has_keyframes_for(clip, "text.font_size");
    text_size_slider(store.clone(), clip_id, &t.style, has_kf_size, children);

    text_style_row(store.clone(), clip_id, &t.style, children);
    text_color_picker(store.clone(), clip_id, &t.style, children);

    font_row(store.clone(), clip_id, &t.style, children);

    children.push(section_header("Transitions"));
    children.push(transition_selector(
        store.clone(),
        clip_id,
        clip.transition_in.as_ref(),
        true,
    ));
    children.push(transition_selector(
        store.clone(),
        clip_id,
        clip.transition_out.as_ref(),
        false,
    ));

    keyframe_section(store.clone(), clip, children);
}

fn subtitle_clip_properties(
    store: Rc<Store>,
    clip: &Clip,
    s: &SubtitleClip,
    children: &mut Vec<View>,
) {
    let clip_id = clip.id;

    children.push(section_header("Subtitle"));
    children.push(kv("Source", &s.source_path));
    if let Some(fp) = &s.font_path {
        children.push(kv("Font", fp));
    }

    children.push(section_header("Properties"));
    let has_kf = clip_has_keyframes_for(clip, "opacity");
    let kf_store = store.clone();
    let kf_cid = clip_id;
    children.push(slider_row_with_kf(
        "Opacity",
        clip.opacity,
        0.0, 1.0, None,
        fmt_pct,
        {
            let store = store.clone();
            let cid = clip_id;
            move |v| store.dispatch_edit(EditCommand::SetClipOpacity {
                clip_id: cid,
                opacity: v,
            })
        },
        has_kf,
        {
            let store = kf_store;
            let cid = kf_cid;
            move || {
                let playhead = store.state.playhead.get();
                store.dispatch_edit(EditCommand::AddKeyframe {
                    clip_id: cid,
                    keyframe: Keyframe {
                        param: "opacity".into(),
                        offset: MediaDuration(playhead.0),
                        value: store.state.timeline.get().and_then(|tl| {
                            tl.tracks.iter().find_map(|t| t.clip_by_id(cid).map(|c| c.opacity))
                        }).unwrap_or(1.0),
                        easing: Easing::Linear,
                    },
                })
            }
        },
    ));

    children.push(section_header("Transitions"));
    children.push(transition_selector(
        store.clone(),
        clip_id,
        clip.transition_in.as_ref(),
        true,
    ));
    children.push(transition_selector(
        store.clone(),
        clip_id,
        clip.transition_out.as_ref(),
        false,
    ));

    keyframe_section(store.clone(), clip, children);
}

fn transition_selector(
    store: Rc<Store>,
    clip_id: ClipId,
    transition: Option<&Transition>,
    is_in: bool,
) -> View {
    let th = theme();
    let dir = if is_in { "in" } else { "out" };
    let label = if is_in { "Transition In" } else { "Transition Out" };
    let drop_key = format!("td:{}:{dir}", clip_id.0);

    let status = match transition {
        Some(t) => format!("{} ({}ms)", transition_kind_label(&t.kind), t.duration.as_micros() / 1000),
        None => "None".to_string(),
    };

    let dropdown_open = flag_get(&drop_key);

    Column(Modifier::new().fill_max_width()).child((
        Row(Modifier::new()
            .fill_max_width()
            .height(28.0)
            .align_items(AlignItems::CENTER))
        .child(vec![
            Text(label).size(10.0).color(th.on_surface_variant),
            Box(Modifier::new().flex_grow(1.0)),
            material3::TextButton(
                Modifier::new().height(22.0),
                {
                    let dk = drop_key.clone();
                    move || flag_toggle(&dk)
                },
                Default::default(),
                move || {
                    Row(Modifier::new().align_items(AlignItems::CENTER)).child((
                        Text(&status).size(10.0).color(th.on_surface),
                        h_spacer(2.0),
                        Icon(if dropdown_open { Icons::arrow_upward } else { Icons::arrow_downward })
                            .size(10.0),
                    ))
                },
            ),
        ]),
        if dropdown_open {
            transition_dropdown_options(store.clone(), clip_id, is_in, transition, &drop_key)
        } else {
            Box(Modifier::new().height(0.0))
        },
    ))
}

fn transition_kind_label(kind: &TransitionKind) -> &'static str {
    match kind {
        TransitionKind::CrossFade => "Cross Fade",
        TransitionKind::SlideLeft => "Slide Left",
        TransitionKind::SlideRight => "Slide Right",
        TransitionKind::Dissolve => "Dissolve",
        _ => "Custom",
    }
}

struct TransitionOption {
    kind: Option<TransitionKind>,
    label: &'static str,
    default_duration_ms: i64,
}

const TRANSITION_OPTIONS: &[TransitionOption] = &[
    TransitionOption { kind: None, label: "None", default_duration_ms: 0 },
    TransitionOption { kind: Some(TransitionKind::CrossFade), label: "Cross Fade", default_duration_ms: 500 },
    TransitionOption { kind: Some(TransitionKind::Dissolve), label: "Dissolve", default_duration_ms: 500 },
    TransitionOption { kind: Some(TransitionKind::SlideLeft), label: "Slide Left", default_duration_ms: 500 },
    TransitionOption { kind: Some(TransitionKind::SlideRight), label: "Slide Right", default_duration_ms: 500 },
];

fn transition_dropdown_options(
    store: Rc<Store>,
    clip_id: ClipId,
    is_in: bool,
    current: Option<&Transition>,
    drop_key: &str,
) -> View {
    let th = theme();
    let dk = drop_key.to_owned();
    let current_kind = current.map(|t| t.kind);
    let current_dur = current.map(|t| t.duration.as_micros()).unwrap_or(500_000) as f32 / 1_000_000.0;

    let mut items: Vec<View> = Vec::new();

    let store_c = store.clone();
    let dk_c = dk.clone();
    for opt in TRANSITION_OPTIONS {
        let opt_store = store_c.clone();
        let opt_dk = dk_c.clone();
        let cid = clip_id;
        let kind = opt.kind;
        let selected = current_kind == opt.kind;
        items.push(
            Row(Modifier::new()
                .fill_max_width()
                .height(24.0)
                .padding_values(PaddingValues { left: 8.0, right: 8.0, top: 0.0, bottom: 0.0 })
                .background(if selected { th.primary.with_alpha(30) } else { Color(0, 0, 0, 0) })
                .clickable()
                .on_pointer_down(move |_| {
                    flag_set(&opt_dk, false);
                    let new_transition = kind.map(|k| {
                        Transition::new(k, MediaDuration::from_micros(
                            if k == TransitionKind::CrossFade || k == TransitionKind::Dissolve {
                                500_000
                            } else {
                                500_000
                            },
                        ))
                    });
                    opt_store.dispatch_edit(if is_in {
                        EditCommand::SetTransitionIn {
                            clip_id: cid,
                            transition: new_transition,
                        }
                    } else {
                        EditCommand::SetTransitionOut {
                            clip_id: cid,
                            transition: new_transition,
                        }
                    });
                }))
            .child(Text(opt.label).size(10.0).color(th.on_surface)),
        );
    }

    if current.is_some() {
        items.push(v_spacer(4.0));
        let store_dur = store.clone();
        let cid = clip_id;
        items.push(slider_row(
            "Duration (s)",
            current_dur,
            0.1, 3.0, Some(0.1),
            |v| format!("{:.1}s", v),
            move |v| {
                let us = (v * 1_000_000.0) as i64;
                let kind = store_dur.state.timeline.get().and_then(|tl| {
                    tl.tracks.iter().find_map(|t| {
                        t.clip_by_id(cid).and_then(|c| {
                            let t = if is_in { &c.transition_in } else { &c.transition_out };
                            t.clone()
                        })
                    })
                }).map(|t| t.kind).unwrap_or(TransitionKind::CrossFade);
                store_dur.dispatch_edit(if is_in {
                    EditCommand::SetTransitionIn {
                        clip_id: cid,
                        transition: Some(Transition::new(kind, MediaDuration::from_micros(us))),
                    }
                } else {
                    EditCommand::SetTransitionOut {
                        clip_id: cid,
                        transition: Some(Transition::new(kind, MediaDuration::from_micros(us))),
                    }
                });
            },
        ));
    }

    Column(Modifier::new()
        .fill_max_width()
        .background(th.surface_variant.with_alpha(120))
        .border(1.0, th.outline.with_alpha(80), 4.0)
        .padding(4.0))
    .child(items)
}

fn filter_row(
    store: Rc<Store>,
    clip_id: ClipId,
    idx: usize,
    effect: &VideoEffect,
    children: &mut Vec<View>,
) {
    let th = theme();
    let name = filter_label(&effect.filter);
    let enabled = effect.enabled;
    let fg = if enabled { th.on_surface } else { th.on_surface_variant.with_alpha(160) };

    let expand_key = format!("fp:{}:{idx}", clip_id.0);
    let expanded = flag_get(&expand_key);
    let has_params = !filter_param_sliders(&effect.filter).is_empty();

    children.push(
        Row(Modifier::new()
            .fill_max_width()
            .height(26.0)
            .align_items(AlignItems::CENTER))
        .child(vec![
            if has_params {
                let ek = expand_key.clone();
                Box(Modifier::new()
                    .clickable()
                    .on_pointer_down(move |_| flag_toggle(&ek)))
                .child(
                    Icon(if expanded { Icons::arrow_downward } else { Icons::arrow_upward })
                        .size(10.0)
                        .color(th.on_surface_variant),
                )
            } else {
                h_spacer(12.0)
            },
            h_spacer(4.0),
            Box(Modifier::new()
                .size(14.0, 14.0)
                .background(if enabled { th.primary } else { th.surface_variant })
                .clip_rounded(2.0)
                .clickable()
                .on_pointer_down({
                    let store = store.clone();
                    move |_| {
                        store.dispatch_edit(EditCommand::SetVideoFilterEnabled {
                            clip_id,
                            index: idx,
                            enabled: !enabled,
                        });
                    }
                })),
            h_spacer(6.0),
            Text(name).size(10.0).color(fg).modifier(Modifier::new().flex_grow(1.0)),
            material3::IconButton(
                Icon(Icons::arrow_upward).size(12.0),
                {
                    let store = store.clone();
                    move || {
                        if idx > 0 {
                            store.dispatch_edit(EditCommand::MoveVideoFilter {
                                clip_id,
                                from: idx,
                                to: idx - 1,
                            });
                        }
                    }
                },
                Default::default(),
            ),
            material3::IconButton(
                Icon(Icons::arrow_downward).size(12.0),
                {
                    let store = store.clone();
                    move || {
                        store.dispatch_edit(EditCommand::MoveVideoFilter {
                            clip_id,
                            from: idx,
                            to: idx + 1,
                        });
                    }
                },
                Default::default(),
            ),
            material3::IconButton(
                Icon(Icons::close).size(12.0),
                {
                    let store = store.clone();
                    move || {
                        store.dispatch_edit(EditCommand::RemoveVideoFilter {
                            clip_id,
                            index: idx,
                        });
                    }
                },
                Default::default(),
            ),
        ]),
    );

    if expanded && has_params {
        filter_param_editors(store, clip_id, idx, effect, children);
    }
}

fn filter_param_editors(
    store: Rc<Store>,
    clip_id: ClipId,
    idx: usize,
    effect: &VideoEffect,
    children: &mut Vec<View>,
) {
    let params = filter_param_sliders(&effect.filter);
    let params_len = params.len();

    children.push(Box(Modifier::new()
        .fill_max_width()
        .padding_values(PaddingValues { left: 12.0, right: 4.0, top: 0.0, bottom: 0.0 }))
    .child(Column(Modifier::new().fill_max_width()).child({
        let mut slider_views: Vec<View> = Vec::new();
        for (pi, pdef) in params.iter().enumerate() {
            let store_clone = store.clone();
            let cid = clip_id;
            let effect_clone = effect.clone();
            let label = pdef.label;
            let val = pdef.value;
            let min = pdef.min;
            let max = pdef.max;
            let step = pdef.step;
            slider_views.push(slider_row(
                label,
                val,
                min, max, step,
                |v| format!("{:.2}", v),
                move |new_val| {
                    let new_filter = filter_apply_value(&effect_clone.filter, pi, new_val);
                    store_clone.dispatch_edit(EditCommand::UpdateVideoFilter {
                        clip_id: cid,
                        index: idx,
                        filter: VideoEffect {
                            enabled: effect_clone.enabled,
                            filter: new_filter,
                        },
                    });
                },
            ));
        }
        slider_views
    })));
}

fn add_filter_dropdown(
    store: Rc<Store>,
    clip_id: ClipId,
    children: &mut Vec<View>,
) {
    let th = theme();
    let drop_key = format!("af:{}", clip_id.0);
    let open = flag_get(&drop_key);

    let filters: &[(&str, fn() -> VideoFilter)] = &[
        ("Brightness", || VideoFilter::Brightness { value: 0.0 }),
        ("Contrast", || VideoFilter::Contrast { value: 1.0 }),
        ("Saturation", || VideoFilter::Saturation { value: 1.0 }),
        ("Grayscale", || VideoFilter::Grayscale),
        ("Blur", || VideoFilter::Blur { radius: 0.0 }),
        ("Sharpen", || VideoFilter::Sharpen { amount: 0.0 }),
        ("Sepia", || VideoFilter::Sepia),
        ("Hue", || VideoFilter::Hue { degrees: 0.0 }),
        ("Crop", || VideoFilter::Crop { left: 0.0, top: 0.0, right: 1.0, bottom: 1.0 }),
        ("Rotate", || VideoFilter::Rotate { degrees: 0.0 }),
        ("Flip", || VideoFilter::Flip { horizontal: false, vertical: false }),
        ("Transform", || VideoFilter::Transform { scale: 1.0, translate_x: 0.0, translate_y: 0.0, rotate: 0.0 }),
        ("Speed", || VideoFilter::Speed { factor: 1.0 }),
        ("Opacity", || VideoFilter::Opacity { value: 1.0 }),
        ("Blend Mode", || VideoFilter::BlendMode { mode: BlendMode::Normal }),
    ];

    children.push(
        material3::TextButton(
            Modifier::new().height(28.0),
            {
                let dk = drop_key.clone();
                move || flag_toggle(&dk)
            },
            Default::default(),
            move || {
                Row(Modifier::new().align_items(AlignItems::CENTER)).child((
                    Icon(Icons::add).size(12.0),
                    h_spacer(4.0),
                    Text("Add Filter").size(10.0),
                ))
            },
        ),
    );

    if open {
        children.push(
            Column(Modifier::new()
                .fill_max_width()
                .background(th.surface_variant.with_alpha(120))
                .border(1.0, th.outline.with_alpha(80), 4.0)
                .padding(4.0))
            .child({
                let mut rows: Vec<View> = Vec::new();
                for (i, (name, ctor)) in filters.iter().enumerate() {
                    let store_c = store.clone();
                    let dk = drop_key.clone();
                    let cid = clip_id;
                    let f = ctor();
                    rows.push(
                        Row(Modifier::new()
                            .fill_max_width()
                            .height(22.0)
                            .padding_values(PaddingValues { left: 8.0, right: 8.0, top: 0.0, bottom: 0.0 })
                            .background(if i % 2 == 0 { th.surface.with_alpha(60) } else { Color(0, 0, 0, 0) })
                            .clickable()
                            .on_pointer_down(move |_| {
                                flag_set(&dk, false);
                                store_c.dispatch_edit(EditCommand::AddVideoFilter {
                                    clip_id: cid,
                                    filter: VideoEffect::new(f.clone()),
                                });
                            }))
                        .child(Text(*name).size(10.0).color(th.on_surface)),
                    );
                }
                rows
            }),
        );
    }
}

fn mask_section(
    store: Rc<Store>,
    clip_id: ClipId,
    masks: &[MaskEffect],
    children: &mut Vec<View>,
) {
    children.push(section_header("Masks"));

    for (idx, mask) in masks.iter().enumerate() {
        let expand_key = format!("me:{}:{idx}", clip_id.0);
        let expanded = flag_get(&expand_key);

        let enabled = mask.enabled;
        let th = theme();
        let shape_label = match &mask.source {
            MaskSource::Shape { shape, .. } => match shape {
                MaskShape::Rectangle { .. } => "Rectangle",
                MaskShape::Ellipse { .. } => "Ellipse",
                _ => "Shape",
            },
            _ => "Shape",
        };

        children.push(
            Row(Modifier::new()
                .fill_max_width()
                .height(26.0)
                .align_items(AlignItems::CENTER))
            .child(vec![
                Box(Modifier::new()
                    .clickable()
                    .on_pointer_down({
                        let ek = expand_key.clone();
                        move |_| flag_toggle(&ek)
                    }))
                .child(
                    Icon(if expanded { Icons::arrow_downward } else { Icons::arrow_upward })
                        .size(10.0),
                ),
                h_spacer(4.0),
                Box(Modifier::new()
                    .size(14.0, 14.0)
                    .background(if enabled { th.primary } else { th.surface_variant })
                    .clip_rounded(2.0)
                    .clickable()
                    .on_pointer_down({
                        let store = store.clone();
                        move |_| {
                            store.dispatch_edit(EditCommand::SetMaskEnabled {
                                clip_id,
                                index: idx,
                                enabled: !enabled,
                            });
                        }
                    })),
                h_spacer(6.0),
                Text(shape_label).size(10.0).color(th.on_surface)
                    .modifier(Modifier::new().flex_grow(1.0)),
                material3::IconButton(
                    Icon(Icons::close).size(12.0),
                    {
                        let store = store.clone();
                        move || {
                            store.dispatch_edit(EditCommand::RemoveMask {
                                clip_id,
                                index: idx,
                            });
                        }
                    },
                    Default::default(),
                ),
            ]),
        );

        if expanded {
            mask_editor(store.clone(), clip_id, idx, mask, children);
        }
    }
}

fn mask_editor(
    store: Rc<Store>,
    clip_id: ClipId,
    idx: usize,
    mask: &MaskEffect,
    children: &mut Vec<View>,
) {
    let th = theme();

    children.push(Box(Modifier::new()
        .fill_max_width()
        .padding_values(PaddingValues { left: 12.0, right: 4.0, top: 0.0, bottom: 0.0 }))
    .child(Column(Modifier::new().fill_max_width()).child({

        let mut rows: Vec<View> = Vec::new();

        rows.push(v_spacer(4.0));

        // Shape type selector (Rectangle / Ellipse)
        let is_rect = matches!(mask.source, MaskSource::Shape { shape: MaskShape::Rectangle { .. }, .. });
        rows.push(
            Row(Modifier::new().fill_max_width().height(24.0).align_items(AlignItems::CENTER))
            .child(vec![
                Text("Shape").size(10.0).color(th.on_surface_variant),
                h_spacer(8.0),
                chip_button("Rect", is_rect, {
                    let store = store.clone();
                    let cid = clip_id;
                    move || {
                        let new_mask = MaskEffect {
                            enabled: true,
                            source: MaskSource::Shape {
                                shape: MaskShape::Rectangle { left: 0.0, top: 0.0, right: 1.0, bottom: 1.0 },
                                feather: 0.0,
                                invert: false,
                            },
                            operation: MaskOperation::Alpha,
                            composition: MaskComposition::Replace,
                            transform: MaskTransform::default(),
                        };
                        store.dispatch_edit(EditCommand::UpdateMask {
                            clip_id: cid,
                            index: idx,
                            mask: new_mask,
                        });
                    }
                }),
                h_spacer(4.0),
                chip_button("Ellipse", !is_rect, {
                    let store = store.clone();
                    let cid = clip_id;
                    move || {
                        let new_mask = MaskEffect {
                            enabled: true,
                            source: MaskSource::Shape {
                                shape: MaskShape::Ellipse { center_x: 0.5, center_y: 0.5, radius_x: 0.4, radius_y: 0.4 },
                                feather: 0.0,
                                invert: false,
                            },
                            operation: MaskOperation::Alpha,
                            composition: MaskComposition::Replace,
                            transform: MaskTransform::default(),
                        };
                        store.dispatch_edit(EditCommand::UpdateMask {
                            clip_id: cid,
                            index: idx,
                            mask: new_mask,
                        });
                    }
                }),
            ]),
        );

        // Transform sliders
        rows.push(slider_row(
            "Scale",
            mask.transform.scale,
            0.0, 4.0, Some(0.01),
            |v| format!("{:.2}", v),
            {
                let store = store.clone();
                let cid = clip_id;
                let m = mask.clone();
                move |v| {
                    let mut new_mask = m.clone();
                    new_mask.transform.scale = v;
                    store.dispatch_edit(EditCommand::UpdateMask {
                        clip_id: cid,
                        index: idx,
                        mask: new_mask,
                    });
                }
            },
        ));
        rows.push(slider_row(
            "Pan X",
            mask.transform.translate_x,
            -2000.0, 2000.0, Some(1.0),
            |v| format!("{:.0}", v),
            {
                let store = store.clone();
                let cid = clip_id;
                let m = mask.clone();
                move |v| {
                    let mut new_mask = m.clone();
                    new_mask.transform.translate_x = v;
                    store.dispatch_edit(EditCommand::UpdateMask {
                        clip_id: cid,
                        index: idx,
                        mask: new_mask,
                    });
                }
            },
        ));
        rows.push(slider_row(
            "Pan Y",
            mask.transform.translate_y,
            -2000.0, 2000.0, Some(1.0),
            |v| format!("{:.0}", v),
            {
                let store = store.clone();
                let cid = clip_id;
                let m = mask.clone();
                move |v| {
                    let mut new_mask = m.clone();
                    new_mask.transform.translate_y = v;
                    store.dispatch_edit(EditCommand::UpdateMask {
                        clip_id: cid,
                        index: idx,
                        mask: new_mask,
                    });
                }
            },
        ));
        rows.push(slider_row(
            "Rotate",
            mask.transform.rotate,
            -180.0, 180.0, Some(0.5),
            |v| format!("{:.1}°", v),
            {
                let store = store.clone();
                let cid = clip_id;
                let m = mask.clone();
                move |v| {
                    let mut new_mask = m.clone();
                    new_mask.transform.rotate = v;
                    store.dispatch_edit(EditCommand::UpdateMask {
                        clip_id: cid,
                        index: idx,
                        mask: new_mask,
                    });
                }
            },
        ));

        // Operation selector
        rows.push(v_spacer(4.0));
        rows.push(
            Row(Modifier::new().fill_max_width().height(24.0).align_items(AlignItems::CENTER))
            .child(vec![
                Text("Operation").size(10.0).color(th.on_surface_variant),
                h_spacer(8.0),
                chip_button("Alpha", mask.operation == MaskOperation::Alpha, {
                    let store = store.clone();
                    let cid = clip_id;
                    let m = mask.clone();
                    move || {
                        let mut new_mask = m.clone();
                        new_mask.operation = MaskOperation::Alpha;
                        store.dispatch_edit(EditCommand::UpdateMask {
                            clip_id: cid,
                            index: idx,
                            mask: new_mask,
                        });
                    }
                }),
                h_spacer(4.0),
                chip_button("Luma", mask.operation == MaskOperation::Luma, {
                    let store = store.clone();
                    let cid = clip_id;
                    let m = mask.clone();
                    move || {
                        let mut new_mask = m.clone();
                        new_mask.operation = MaskOperation::Luma;
                        store.dispatch_edit(EditCommand::UpdateMask {
                            clip_id: cid,
                            index: idx,
                            mask: new_mask,
                        });
                    }
                }),
            ]),
        );
        rows.push(
            Row(Modifier::new().fill_max_width().height(24.0).align_items(AlignItems::CENTER))
            .child(vec![
                h_spacer(44.0),
                chip_button("Invert α", mask.operation == MaskOperation::InvertAlpha, {
                    let store = store.clone();
                    let cid = clip_id;
                    let m = mask.clone();
                    move || {
                        let mut new_mask = m.clone();
                        new_mask.operation = MaskOperation::InvertAlpha;
                        store.dispatch_edit(EditCommand::UpdateMask {
                            clip_id: cid,
                            index: idx,
                            mask: new_mask,
                        });
                    }
                }),
                h_spacer(4.0),
                chip_button("Invert Luma", mask.operation == MaskOperation::InvertLuma, {
                    let store = store.clone();
                    let cid = clip_id;
                    let m = mask.clone();
                    move || {
                        let mut new_mask = m.clone();
                        new_mask.operation = MaskOperation::InvertLuma;
                        store.dispatch_edit(EditCommand::UpdateMask {
                            clip_id: cid,
                            index: idx,
                            mask: new_mask,
                        });
                    }
                }),
            ]),
        );

        // Composition selector
        rows.push(v_spacer(4.0));
        rows.push(
            Row(Modifier::new().fill_max_width().height(24.0).align_items(AlignItems::CENTER))
            .child(vec![
                Text("Composition").size(10.0).color(th.on_surface_variant),
                h_spacer(8.0),
                chip_button("Replace", mask.composition == MaskComposition::Replace, {
                    let store = store.clone();
                    let cid = clip_id;
                    let m = mask.clone();
                    move || {
                        let mut new_mask = m.clone();
                        new_mask.composition = MaskComposition::Replace;
                        store.dispatch_edit(EditCommand::UpdateMask {
                            clip_id: cid,
                            index: idx,
                            mask: new_mask,
                        });
                    }
                }),
                h_spacer(4.0),
                chip_button("Union", mask.composition == MaskComposition::Union, {
                    let store = store.clone();
                    let cid = clip_id;
                    let m = mask.clone();
                    move || {
                        let mut new_mask = m.clone();
                        new_mask.composition = MaskComposition::Union;
                        store.dispatch_edit(EditCommand::UpdateMask {
                            clip_id: cid,
                            index: idx,
                            mask: new_mask,
                        });
                    }
                }),
            ]),
        );
        rows.push(
            Row(Modifier::new().fill_max_width().height(24.0).align_items(AlignItems::CENTER))
            .child(vec![
                h_spacer(44.0),
                chip_button("Intersect", mask.composition == MaskComposition::Intersect, {
                    let store = store.clone();
                    let cid = clip_id;
                    let m = mask.clone();
                    move || {
                        let mut new_mask = m.clone();
                        new_mask.composition = MaskComposition::Intersect;
                        store.dispatch_edit(EditCommand::UpdateMask {
                            clip_id: cid,
                            index: idx,
                            mask: new_mask,
                        });
                    }
                }),
                h_spacer(4.0),
                chip_button("Subtract", mask.composition == MaskComposition::Subtract, {
                    let store = store.clone();
                    let cid = clip_id;
                    let m = mask.clone();
                    move || {
                        let mut new_mask = m.clone();
                        new_mask.composition = MaskComposition::Subtract;
                        store.dispatch_edit(EditCommand::UpdateMask {
                            clip_id: cid,
                            index: idx,
                            mask: new_mask,
                        });
                    }
                }),
            ]),
        );

        // Feather slider
        if let MaskSource::Shape { feather, invert, .. } = &mask.source {
            rows.push(slider_row(
                "Feather",
                *feather,
                0.0, 0.2, Some(0.01),
                |v| format!("{:.3}", v),
                {
                    let store = store.clone();
                    let cid = clip_id;
                    let m = mask.clone();
                    move |v| {
                        let mut new_mask = m.clone();
                        if let MaskSource::Shape { feather, .. } = &mut new_mask.source {
                            *feather = v;
                        }
                        store.dispatch_edit(EditCommand::UpdateMask {
                            clip_id: cid,
                            index: idx,
                            mask: new_mask,
                        });
                    }
                },
            ));

            // Invert toggle
            rows.push(
                Row(Modifier::new().fill_max_width().height(24.0).align_items(AlignItems::CENTER))
                .child(vec![
                    Text("Invert").size(10.0).color(th.on_surface_variant),
                    h_spacer(8.0),
                    chip_button("On", *invert, {
                        let store = store.clone();
                        let cid = clip_id;
                        let m = mask.clone();
                        move || {
                            let mut new_mask = m.clone();
                            if let MaskSource::Shape { invert, .. } = &mut new_mask.source {
                                *invert = true;
                            }
                            store.dispatch_edit(EditCommand::UpdateMask {
                                clip_id: cid,
                                index: idx,
                                mask: new_mask,
                            });
                        }
                    }),
                    h_spacer(4.0),
                    chip_button("Off", !*invert, {
                        let store = store.clone();
                        let cid = clip_id;
                        let m = mask.clone();
                        move || {
                            let mut new_mask = m.clone();
                            if let MaskSource::Shape { invert, .. } = &mut new_mask.source {
                                *invert = false;
                            }
                            store.dispatch_edit(EditCommand::UpdateMask {
                                clip_id: cid,
                                index: idx,
                                mask: new_mask,
                            });
                        }
                    }),
                ]),
            );
        }

        // Add mask button
        rows.push(v_spacer(4.0));
        let add_store = store.clone();
        let add_cid = clip_id;
        rows.push(
            material3::TextButton(
                Modifier::new().height(24.0),
                move || {
                    add_store.dispatch_edit(EditCommand::AddMask {
                        clip_id: add_cid,
                        mask: MaskEffect::shape(MaskShape::Rectangle {
                            left: 0.0, top: 0.0, right: 1.0, bottom: 1.0,
                        }),
                    });
                },
                Default::default(),
                || {
                    Row(Modifier::new().align_items(AlignItems::CENTER)).child((
                        Icon(Icons::add).size(10.0),
                        h_spacer(4.0),
                        Text("Add Mask").size(10.0),
                    ))
                },
            ),
        );

        rows
    })));
}

fn chip_button(label: &str, selected: bool, on_click: impl Fn() + 'static) -> View {
    let th = theme();
    if selected {
        material3::FilledTonalButton(
            Modifier::new().height(20.0),
            on_click,
            Default::default(),
            move || Text(label).size(9.0),
        )
    } else {
        material3::TextButton(
            Modifier::new().height(20.0),
            on_click,
            Default::default(),
            move || Text(label).size(9.0).color(th.on_surface_variant),
        )
    }
}

fn keyframe_section(store: Rc<Store>, clip: &Clip, children: &mut Vec<View>) {
    let th = theme();
    let clip_id = clip.id;
    let kfs = &clip.keyframes.keyframes;

    if kfs.is_empty() {
        return;
    }

    let mut groups: HashMap<String, Vec<(usize, Keyframe)>> = HashMap::new();
    for (i, kf) in kfs.iter().enumerate() {
        groups.entry(kf.param.clone()).or_default().push((i, kf.clone()));
    }

    children.push(section_header("Keyframes"));

    let mut param_keys: Vec<&String> = groups.keys().collect();
    param_keys.sort();

    for param in &param_keys {
        let entries = &groups[param as &str];
        let group_key = format!("kg:{}:{param}", clip_id.0);
        let expanded = flag_get(&group_key);

        children.push(keyframe_group_header(
            store.clone(),
            clip,
            param,
            entries.len(),
            &group_key,
            expanded,
        ));

        if expanded {
            keyframe_entries(store.clone(), clip_id, entries, children);
        }
    }
}

fn keyframe_group_header(
    store: Rc<Store>,
    clip: &Clip,
    param: &str,
    count: usize,
    group_key: &str,
    expanded: bool,
) -> View {
    let th = theme();
    let gk = group_key.to_owned();
    let param_owned = param.to_owned();
    let clip_id = clip.id;

    Row(Modifier::new()
        .fill_max_width()
        .height(26.0)
        .align_items(AlignItems::CENTER))
    .child(vec![
        Box(Modifier::new()
            .clickable()
            .on_pointer_down(move |_| flag_toggle(&gk)))
        .child(
            Icon(if expanded { Icons::arrow_downward } else { Icons::arrow_upward })
                .size(10.0),
        ),
        h_spacer(4.0),
        Text(param_label(param))
            .size(10.0)
            .color(th.on_surface)
            .modifier(Modifier::new().flex_grow(1.0)),
        Box(Modifier::new()
            .background(th.primary)
            .clip_rounded(8.0)
            .padding_values(PaddingValues { left: 5.0, right: 5.0, top: 1.0, bottom: 1.0 }))
        .child(Text(format!("{}", count)).size(8.0).color(th.on_primary)),
        material3::IconButton(
            Icon(Icons::diamond).size(12.0),
            {
                let store = store.clone();
                let cid = clip_id;
                let p = param_owned.clone();
                let clip_opacity = clip.opacity;
                let clip_volume = clip.volume;
                let clip_speed = clip.speed as f32;
                let text_pos_x = if let ClipKind::Text(t) = &clip.kind { t.style.position_x } else { 0.5 };
                let text_pos_y = if let ClipKind::Text(t) = &clip.kind { t.style.position_y } else { 0.9 };
                let text_size = if let ClipKind::Text(t) = &clip.kind { t.style.font_size } else { 24.0 };
                move || {
                    let val = match p.as_str() {
                        "opacity" => clip_opacity,
                        "volume" => clip_volume,
                        "speed" => clip_speed,
                        "text.position_x" => text_pos_x,
                        "text.position_y" => text_pos_y,
                        "text.font_size" => text_size,
                        _ => 0.0,
                    };
                    store.dispatch_edit(EditCommand::AddKeyframe {
                        clip_id: cid,
                        keyframe: Keyframe {
                            param: p.clone(),
                            offset: MediaDuration(store.state.playhead.get().0),
                            value: val,
                            easing: Easing::Linear,
                        },
                    });
                }
            },
            Default::default(),
        ),
    ])
}

fn param_label(param: &str) -> String {
    match param {
        "opacity" => "Opacity".into(),
        "volume" => "Volume".into(),
        "speed" => "Speed".into(),
        "transform.scale" => "Scale".into(),
        "transform.translate_x" => "Pan X".into(),
        "transform.translate_y" => "Pan Y".into(),
        "transform.rotate" => "Rotate".into(),
        "text.position_x" => "Position X".into(),
        "text.position_y" => "Position Y".into(),
        "text.font_size" => "Font Size".into(),
        "mask.feather" => "Feather".into(),
        "mask.scale" => "Mask Scale".into(),
        "mask.translate_x" => "Mask Pan X".into(),
        "mask.translate_y" => "Mask Pan Y".into(),
        "mask.rotate" => "Mask Rotate".into(),
        _ => param.to_string(),
    }
}

fn param_bounds(param: &str) -> (f32, f32) {
    match param {
        "opacity" => (0.0, 1.0),
        "volume" => (0.0, 2.0),
        "speed" => (0.1, 4.0),
        "transform.scale" | "mask.scale" => (0.0, 4.0),
        "transform.translate_x" | "mask.translate_x" => (-2000.0, 2000.0),
        "transform.translate_y" | "mask.translate_y" => (-2000.0, 2000.0),
        "transform.rotate" | "mask.rotate" => (-180.0, 180.0),
        "text.position_x" | "text.position_y" => (0.0, 1.0),
        "text.font_size" => (8.0, 200.0),
        "mask.feather" => (0.0, 0.2),
        _ => (0.0, 1.0),
    }
}

fn keyframe_entries(
    store: Rc<Store>,
    clip_id: ClipId,
    entries: &[(usize, Keyframe)],
    children: &mut Vec<View>,
) {
    let th = theme();

    for (abs_idx, kf) in entries {
        let edit_key = format!("ke:{}:{}:{}", clip_id.0, kf.param, abs_idx);
        let editing = flag_get(&edit_key);

        let idx = *abs_idx;
        let kf_param = kf.param.clone();
        let kf_offset = kf.offset;
        let kf_value = kf.value;
        let kf_easing = kf.easing;
        let offset_sec = kf.offset.as_micros() as f32 / 1_000_000.0;
        let val_display = format!("{:.3}", kf.value);
        let easing_label = match kf.easing {
            Easing::Linear => "Lin",
            Easing::EaseIn => "In",
            Easing::EaseOut => "Out",
            Easing::EaseInOut => "InOut",
            _ => "?",
        };

        let time_str = fmt_us(kf.offset.0);

        children.push(
            Row(Modifier::new()
                .fill_max_width()
                .height(22.0)
                .padding_values(PaddingValues { left: 16.0, right: 4.0, top: 0.0, bottom: 0.0 })
                .align_items(AlignItems::CENTER))
            .child(vec![
                Icon(Icons::diamond)
                    .size(8.0)
                    .color(th.primary)
                    .modifier(Modifier::new().clickable().on_pointer_down({ let ek = edit_key.clone(); move |_| flag_toggle(&ek) })),
                h_spacer(4.0),
                Text(&time_str).size(9.0).color(th.on_surface_variant),
                h_spacer(4.0),
                Text(&val_display).size(9.0).color(th.on_surface),
                Box(Modifier::new().flex_grow(1.0)),
                Text(easing_label).size(8.0).color(th.on_surface_variant),
                h_spacer(4.0),
                material3::IconButton(
                    Icon(Icons::close).size(10.0),
                    {
                        let store = store.clone();
                        let cid = clip_id;
                        let abs_idx = idx;
                        move || {
                            store.dispatch_edit(EditCommand::RemoveKeyframe {
                                clip_id: cid,
                                index: abs_idx,
                            });
                        }
                    },
                    Default::default(),
                ),
            ]),
        );

        if editing {
            keyframe_edit_form(store.clone(), clip_id, *abs_idx, kf, &edit_key, children);
        }
    }
}

fn keyframe_edit_form(
    store: Rc<Store>,
    clip_id: ClipId,
    abs_idx: usize,
    kf: &Keyframe,
    edit_key: &str,
    children: &mut Vec<View>,
) {
    let th = theme();
    let ek = edit_key.to_owned();
    let (min_val, max_val) = param_bounds(&kf.param);
    let timeline = store.state.timeline.get();
    let duration_us = timeline.as_ref().and_then(|tl| {
        for track in &tl.tracks {
            if let Some(c) = track.clip_by_id(clip_id) {
                return Some(c.timeline_duration.0 as f32);
            }
        }
        None
    }).unwrap_or(3_000_000.0);
    let max_time_s = duration_us / 1_000_000.0;
    let current_time_s = kf.offset.0 as f32 / 1_000_000.0;
    let easing = kf.easing;

    children.push(Box(Modifier::new()
        .fill_max_width()
        .padding_values(PaddingValues { left: 24.0, right: 4.0, top: 0.0, bottom: 4.0 })
        .background(th.surface_variant.with_alpha(60))
        .border(1.0, th.outline.with_alpha(60), 4.0))
    .child(Column(Modifier::new().fill_max_width()).child(vec![
        slider_row(
            "Time",
            current_time_s,
            0.0, max_time_s, Some(0.01),
            |v| format!("{:.2}s", v),
            {
                let store = store.clone();
                let cid = clip_id;
                let p = kf.param.clone();
                let val = kf.value;
                let e = kf.easing;
                let ek = ek.clone();
                move |v| {
                    let us = (v * 1_000_000.0) as i64;
                    store.dispatch_edit(EditCommand::UpdateKeyframe {
                        clip_id: cid,
                        index: abs_idx,
                        keyframe: Keyframe {
                            param: p.clone(),
                            offset: MediaDuration(us),
                            value: val,
                            easing: e,
                        },
                    });
                }
            },
        ),
        slider_row(
            "Value",
            kf.value,
            min_val, max_val, None,
            |v| format!("{:.3}", v),
            {
                let store = store.clone();
                let cid = clip_id;
                let p = kf.param.clone();
                let off = kf.offset;
                let e = kf.easing;
                move |v| {
                    store.dispatch_edit(EditCommand::UpdateKeyframe {
                        clip_id: cid,
                        index: abs_idx,
                        keyframe: Keyframe {
                            param: p.clone(),
                            offset: off,
                            value: v,
                            easing: e,
                        },
                    });
                }
            },
        ),
        // Easing selector
        Row(Modifier::new().fill_max_width().height(24.0).align_items(AlignItems::CENTER))
        .child(vec![
            Text("Easing").size(10.0).color(th.on_surface_variant),
            h_spacer(8.0),
            chip_button("Linear", easing == Easing::Linear, {
                let store = store.clone();
                let cid = clip_id;
                let p = kf.param.clone();
                let off = kf.offset;
                let val = kf.value;
                move || {
                    store.dispatch_edit(EditCommand::UpdateKeyframe {
                        clip_id: cid,
                        index: abs_idx,
                        keyframe: Keyframe {
                            param: p.clone(),
                            offset: off,
                            value: val,
                            easing: Easing::Linear,
                        },
                    });
                }
            }),
            h_spacer(4.0),
            chip_button("Ease In", easing == Easing::EaseIn, {
                let store = store.clone();
                let cid = clip_id;
                let p = kf.param.clone();
                let off = kf.offset;
                let val = kf.value;
                move || {
                    store.dispatch_edit(EditCommand::UpdateKeyframe {
                        clip_id: cid,
                        index: abs_idx,
                        keyframe: Keyframe {
                            param: p.clone(),
                            offset: off,
                            value: val,
                            easing: Easing::EaseIn,
                        },
                    });
                }
            }),
            h_spacer(4.0),
            chip_button("Ease Out", easing == Easing::EaseOut, {
                let store = store.clone();
                let cid = clip_id;
                let p = kf.param.clone();
                let off = kf.offset;
                let val = kf.value;
                move || {
                    store.dispatch_edit(EditCommand::UpdateKeyframe {
                        clip_id: cid,
                        index: abs_idx,
                        keyframe: Keyframe {
                            param: p.clone(),
                            offset: off,
                            value: val,
                            easing: Easing::EaseOut,
                        },
                    });
                }
            }),
            h_spacer(4.0),
            chip_button("InOut", easing == Easing::EaseInOut, {
                let store = store.clone();
                let cid = clip_id;
                let p = kf.param.clone();
                let off = kf.offset;
                let val = kf.value;
                move || {
                    store.dispatch_edit(EditCommand::UpdateKeyframe {
                        clip_id: cid,
                        index: abs_idx,
                        keyframe: Keyframe {
                            param: p.clone(),
                            offset: off,
                            value: val,
                            easing: Easing::EaseInOut,
                        },
                    });
                }
            }),
         ]),
        ])))
}


fn text_position_grid(
    store: Rc<Store>,
    clip_id: ClipId,
    style: &TextStyle,
    children: &mut Vec<View>,
) {
    let th = theme();
    let positions: [(f32, f32, &str); 9] = [
        (0.05, 0.05, "↖"),
        (0.5, 0.05, "↑"),
        (0.95, 0.05, "↗"),
        (0.05, 0.5, "←"),
        (0.5, 0.5, "•"),
        (0.95, 0.5, "→"),
        (0.05, 0.95, "↙"),
        (0.5, 0.95, "↓"),
        (0.95, 0.95, "↘"),
    ];

    children.push(v_spacer(4.0));
    children.push(Text("Position Preset").size(11.0).color(th.on_surface_variant));

    let mut grid_children: Vec<View> = Vec::new();
    for &(px, py, sym) in &positions {
        let selected = (style.position_x - px).abs() < 0.04 && (style.position_y - py).abs() < 0.04;
        let store = store.clone();
        let cid = clip_id;
        let s = style.clone();
        grid_children.push(
            if selected {
                material3::FilledTonalButton(
                    Modifier::new().size(22.0, 22.0),
                    move || {},
                    Default::default(),
                    move || Text(sym).size(10.0),
                )
            } else {
                material3::TextButton(
                    Modifier::new().size(22.0, 22.0),
                    {
                        let store = store.clone();
                        let cid = clip_id;
                        let s = style.clone();
                        move || {
                            let mut new_style = s.clone();
                            new_style.position_x = px;
                            new_style.position_y = py;
                            store.dispatch_edit(EditCommand::UpdateTextStyle {
                                clip_id: cid,
                                style: new_style,
                            });
                        }
                    },
                    Default::default(),
                    move || Text(sym).size(10.0).color(th.on_surface_variant),
                )
            },
        );
    }

    children.push(Row(Modifier::new()
        .fill_max_width()
        .align_items(AlignItems::CENTER))
    .child({
        let mut cells: Vec<View> = Vec::new();
        for (i, cell) in grid_children.into_iter().enumerate() {
            if i > 0 && i % 3 == 0 {
                cells.push(Box(Modifier::new().width(9999.0).height(0.0)));
            }
            if i > 0 && i % 3 != 0 {
                cells.push(h_spacer(4.0));
            }
            cells.push(cell);
        }
        let rows: Vec<View> = cells.split_off(0);
        Column(Modifier::new().fill_max_width()).child(rows)
    }));

    // Actually, let's do this properly as a 3x3 grid
    // Clear and rebuild
    children.pop(); // remove the incorrect row
    children.pop(); // remove the row above it
    children.pop(); // remove the spacer

    // Redo properly
    children.push(v_spacer(4.0));
    children.push(Text("Position Preset").size(11.0).color(th.on_surface_variant));

    for row in 0..3 {
        let mut row_children: Vec<View> = Vec::new();
        for col in 0..3 {
            let idx = row * 3 + col;
            let (px, py, sym) = positions[idx];
            let selected = (style.position_x - px).abs() < 0.04 && (style.position_y - py).abs() < 0.04;
            if selected {
                let store = store.clone();
                let cid = clip_id;
                let s = style.clone();
                row_children.push(
                    material3::FilledTonalButton(
                        Modifier::new().size(24.0, 24.0),
                        move || {},
                        Default::default(),
                        move || Text(sym).size(11.0),
                    ),
                );
            } else {
                row_children.push(
                    material3::TextButton(
                        Modifier::new().size(24.0, 24.0),
                        {
                            let store = store.clone();
                            let cid = clip_id;
                            let s = style.clone();
                            move || {
                                let mut new_s = s.clone();
                                new_s.position_x = px;
                                new_s.position_y = py;
                                store.dispatch_edit(EditCommand::UpdateTextStyle {
                                    clip_id: cid,
                                    style: new_s,
                                });
                            }
                        },
                        Default::default(),
                        move || Text(sym).size(11.0).color(th.on_surface_variant),
                    ),
                );
            }
            if col < 2 {
                row_children.push(h_spacer(4.0));
            }
        }
        children.push(Row(Modifier::new().fill_max_width()).child(row_children));
    }
}

fn text_position_sliders(
    store: Rc<Store>,
    clip: &Clip,
    style: &TextStyle,
    has_kf_x: bool,
    has_kf_y: bool,
    children: &mut Vec<View>,
) {
    let clip_id = clip.id;
    let pos_x = if let ClipKind::Text(t) = &clip.kind { t.style.position_x } else { 0.5 };
    let pos_y = if let ClipKind::Text(t) = &clip.kind { t.style.position_y } else { 0.9 };

    let kf_x_store = store.clone();
    let kf_x_cid = clip_id;
    children.push(slider_row_with_kf(
        "Position X",
        style.position_x,
        0.0, 1.0, Some(0.01),
        |v| format!("{:.0}%", v * 100.0),
        {
            let store = store.clone();
            let cid = clip_id;
            let s = style.clone();
            move |v| {
                let mut new_s = s.clone();
                new_s.position_x = v;
                store.dispatch_edit(EditCommand::UpdateTextStyle {
                    clip_id: cid,
                    style: new_s,
                });
            }
        },
        has_kf_x,
        {
            let store = kf_x_store;
            let cid = kf_x_cid;
            let val = pos_x;
            move || {
                let playhead = store.state.playhead.get();
                store.dispatch_edit(EditCommand::AddKeyframe {
                    clip_id: cid,
                    keyframe: Keyframe {
                        param: "text.position_x".into(),
                        offset: MediaDuration(playhead.0),
                        value: val,
                        easing: Easing::Linear,
                    },
                });
            }
        },
    ));

    let kf_y_store = store.clone();
    let kf_y_cid = clip_id;
    children.push(slider_row_with_kf(
        "Position Y",
        style.position_y,
        0.0, 1.0, Some(0.01),
        |v| format!("{:.0}%", v * 100.0),
        {
            let store = store.clone();
            let cid = clip_id;
            let s = style.clone();
            move |v| {
                let mut new_s = s.clone();
                new_s.position_y = v;
                store.dispatch_edit(EditCommand::UpdateTextStyle {
                    clip_id: cid,
                    style: new_s,
                });
            }
        },
        has_kf_y,
        {
            let store = kf_y_store;
            let cid = kf_y_cid;
            let val = pos_y;
            move || {
                let playhead = store.state.playhead.get();
                store.dispatch_edit(EditCommand::AddKeyframe {
                    clip_id: cid,
                    keyframe: Keyframe {
                        param: "text.position_y".into(),
                        offset: MediaDuration(playhead.0),
                        value: val,
                        easing: Easing::Linear,
                    },
                });
            }
        },
    ));
}

fn text_size_slider(
    store: Rc<Store>,
    clip_id: ClipId,
    style: &TextStyle,
    has_kf: bool,
    children: &mut Vec<View>,
) {
    let kf_store = store.clone();
    let kf_cid = clip_id;
    children.push(slider_row_with_kf(
        "Font Size",
        style.font_size,
        8.0, 200.0, Some(1.0),
        |v| format!("{:.0}px", v),
        {
            let store = store.clone();
            let cid = clip_id;
            let s = style.clone();
            move |v| {
                let mut new_s = s.clone();
                new_s.font_size = v;
                store.dispatch_edit(EditCommand::UpdateTextStyle {
                    clip_id: cid,
                    style: new_s,
                });
            }
        },
        has_kf,
        {
            let store = kf_store;
            let cid = kf_cid;
            move || {
                let playhead = store.state.playhead.get();
                store.dispatch_edit(EditCommand::AddKeyframe {
                    clip_id: cid,
                    keyframe: Keyframe {
                        param: "text.font_size".into(),
                        offset: MediaDuration(playhead.0),
                        value: 24.0,
                        easing: Easing::Linear,
                    },
                });
            }
        },
    ));
}

fn text_style_row(
    store: Rc<Store>,
    clip_id: ClipId,
    style: &TextStyle,
    children: &mut Vec<View>,
) {
    let th = theme();

    children.push(v_spacer(4.0));
    children.push(Text("Style").size(11.0).color(th.on_surface_variant));

    children.push(
        Row(Modifier::new().fill_max_width().align_items(AlignItems::CENTER))
        .child(vec![
            chip_button("B", style.bold, {
                let store = store.clone();
                let cid = clip_id;
                let s = style.clone();
                move || {
                    let mut new_s = s.clone();
                    new_s.bold = !s.bold;
                    store.dispatch_edit(EditCommand::UpdateTextStyle {
                        clip_id: cid,
                        style: new_s,
                    });
                }
            }),
            h_spacer(4.0),
            chip_button("I", style.italic, {
                let store = store.clone();
                let cid = clip_id;
                let s = style.clone();
                move || {
                    let mut new_s = s.clone();
                    new_s.italic = !s.italic;
                    store.dispatch_edit(EditCommand::UpdateTextStyle {
                        clip_id: cid,
                        style: new_s,
                    });
                }
            }),
            h_spacer(4.0),
            chip_button("BG", style.background_color.is_some(), {
                let store = store.clone();
                let cid = clip_id;
                let s = style.clone();
                move || {
                    let mut new_s = s.clone();
                    new_s.background_color = if s.background_color.is_some() {
                        None
                    } else {
                        Some("00000080".into())
                    };
                    store.dispatch_edit(EditCommand::UpdateTextStyle {
                        clip_id: cid,
                        style: new_s,
                    });
                }
            }),
        ]),
    );
}

fn text_color_picker(
    store: Rc<Store>,
    clip_id: ClipId,
    style: &TextStyle,
    children: &mut Vec<View>,
) {
    let th = theme();

    children.push(v_spacer(4.0));
    children.push(Text("Color").size(11.0).color(th.on_surface_variant));

    let color_hexes: [(&str, Color); 8] = [
        ("FFFFFFFF", Color(255, 255, 255, 255)),
        ("000000FF", Color(0, 0, 0, 255)),
        ("FF0000FF", Color(255, 0, 0, 255)),
        ("FFFF00FF", Color(255, 255, 0, 255)),
        ("00FF00FF", Color(0, 255, 0, 255)),
        ("00FFFFFF", Color(0, 255, 255, 255)),
        ("FF00FFFF", Color(255, 0, 255, 255)),
        ("FF8800FF", Color(255, 136, 0, 255)),
    ];

    let mut swatch_children: Vec<View> = Vec::new();
    for (hex, c) in &color_hexes {
        let selected = style.color.to_uppercase() == *hex;
        swatch_children.push(
            Box(Modifier::new()
                .size(20.0, 20.0)
                .background(*c)
                .border(if selected { 2.0 } else { 1.0 }, if selected { th.primary } else { th.outline }, 10.0)
                .clickable()
                .on_pointer_down({
                    let store = store.clone();
                    let cid = clip_id;
                    let s = style.clone();
                    let hex_str = hex.to_string();
                    move |_| {
                        let mut new_s = s.clone();
                        new_s.color = hex_str.clone();
                        store.dispatch_edit(EditCommand::UpdateTextStyle {
                            clip_id: cid,
                            style: new_s,
                        });
                    }
                })),
        );
        swatch_children.push(h_spacer(6.0));
    }

    children.push(Row(Modifier::new().fill_max_width()).child(swatch_children));
}

fn font_row(
    store: Rc<Store>,
    clip_id: ClipId,
    style: &TextStyle,
    children: &mut Vec<View>,
) {
    let th = theme();

    children.push(v_spacer(4.0));
    children.push(Text("Font").size(11.0).color(th.on_surface_variant));

    children.push(
        Row(Modifier::new()
            .fill_max_width()
            .height(26.0)
            .align_items(AlignItems::CENTER))
        .child(vec![
            Text(&style.font_family)
                .size(10.0)
                .color(th.on_surface)
                .modifier(Modifier::new().flex_grow(1.0)),
            material3::TextButton(
                Modifier::new().height(22.0),
                {
                    let store = store.clone();
                    let cid = clip_id;
                    let s = style.clone();
                    move || {
                        let mut new_s = s.clone();
                        new_s.font_family = "sans-serif".into();
                        store.dispatch_edit(EditCommand::UpdateTextStyle {
                            clip_id: cid,
                            style: new_s,
                        });
                    }
                },
                Default::default(),
                || Text("Reset").size(9.0),
            ),
        ]),
    );

    let font_path_key = format!("font_path:{}", clip_id.0);
    let font_path = str_val_get(&font_path_key).unwrap_or_default();
    children.push(
        Row(Modifier::new()
            .fill_max_width()
            .height(26.0)
            .align_items(AlignItems::CENTER))
        .child(vec![
            Text("Font File").size(10.0).color(th.on_surface_variant),
            h_spacer(4.0),
            Text(if font_path.is_empty() { "(none)" } else { &font_path })
                .size(9.0)
                .color(th.on_surface_variant)
                .modifier(Modifier::new().flex_grow(1.0)),
            material3::TextButton(
                Modifier::new().height(22.0),
                {
                    let store = store.clone();
                    let cid = clip_id;
                    let fpk = font_path_key.clone();
                    move || {
                        let s = str_val_get(&fpk).unwrap_or_default();
                        if !s.is_empty() {
                            store.dispatch_edit(EditCommand::SetSubtitleFont {
                                clip_id: cid,
                                font_path: Some(s),
                            });
                        }
                    }
                },
                Default::default(),
                || Text("Apply").size(9.0),
            ),
        ]),
    );
}

fn audio_filter_row(
    store: Rc<Store>,
    clip_id: ClipId,
    idx: usize,
    filter: &AudioFilter,
    is_video_clip: bool,
    children: &mut Vec<View>,
) {
    let th = theme();
    let name = audio_filter_label(filter);

    let expand_key = format!("afp:{}:{idx}:{}", clip_id.0, is_video_clip);
    let expanded = flag_get(&expand_key);
    let has_duration = matches!(filter, AudioFilter::FadeIn { .. } | AudioFilter::FadeOut { .. });

    children.push(
        Row(Modifier::new()
            .fill_max_width()
            .height(26.0)
            .align_items(AlignItems::CENTER))
        .child(vec![
            if has_duration {
                let ek = expand_key.clone();
                Box(Modifier::new()
                    .clickable()
                    .on_pointer_down(move |_| flag_toggle(&ek)))
                .child(
                    Icon(if expanded { Icons::arrow_downward } else { Icons::arrow_upward })
                        .size(10.0),
                )
            } else {
                h_spacer(12.0)
            },
            h_spacer(4.0),
            Text(name).size(10.0).color(th.on_surface)
                .modifier(Modifier::new().flex_grow(1.0)),
            if let Some(dur_us) = audio_filter_duration_us(filter) {
                Text(fmt_us(dur_us)).size(9.0).color(th.on_surface_variant)
            } else {
                Box(Modifier::new().width(0.0))
            },
            material3::IconButton(
                Icon(Icons::close).size(12.0),
                {
                    let store = store.clone();
                    move || {
                        store.dispatch_edit(EditCommand::RemoveAudioFilter {
                            clip_id,
                            index: idx,
                        });
                    }
                },
                Default::default(),
            ),
        ]),
    );

    if expanded && has_duration {
        let dur_us = audio_filter_duration_us(filter).unwrap_or(1_000_000);
        let store_dur = store.clone();
        let cid = clip_id;

        children.push(Box(Modifier::new()
            .fill_max_width()
            .padding_values(PaddingValues { left: 12.0, right: 4.0, top: 0.0, bottom: 0.0 }))
        .child(slider_row(
            "Duration (s)",
            dur_us as f32 / 1_000_000.0,
            0.1, 5.0, Some(0.1),
            |v| format!("{:.1}s", v),
            move |v| {
                let new_us = (v * 1_000_000.0) as i64;
                store_dur.dispatch_edit(EditCommand::UpdateAudioFilterDuration {
                    clip_id: cid,
                    index: idx,
                    duration_us: new_us,
                });
            },
        )));
    }
}
