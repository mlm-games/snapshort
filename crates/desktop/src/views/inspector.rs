use crate::state::Store;
use miniter_domain::{
    filter::VideoEffect,
    Clip, ClipId, ClipKind, MediaDuration, Timestamp, Track, Transition, TransitionKind,
};
use miniter_usecases::EditCommand;
use repose_core::{
    prelude::theme, Color, Modifier, View,
};
use repose_material::{
    material3,
    Icon,
};
use repose_ui::{
    scroll::{remember_scroll_state, ScrollArea},
    textfield::{set_textfield_state, get_textfield_state},
    BasicTextField, Box, Column, Row, Text, TextFieldConfig, TextFieldState, TextStyle, ViewExt,
};
use snapshort_ui_core::Icons;
use std::rc::Rc;

fn h_spacer(w: f32) -> View {
    Box(Modifier::new().width(w))
}
fn v_spacer(h: f32) -> View {
    Box(Modifier::new().height(h))
}

pub fn inspector_panel(store: Rc<Store>) -> View {
    let th = theme();
    let selected_clip_id = store.state.selected_clip_id.get();
    let selected_asset_id = store.state.selected_asset_id.get();
    let timeline = store.state.timeline.get();
    let assets = store.state.assets.get();

    if let (Some(clip_id), Some(tl)) = (selected_clip_id, timeline.clone()) {
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

fn kv(label: impl Into<String>, value: impl Into<String>) -> View {
    let th = theme();
    Row(Modifier::new()
        .fill_max_width()
        .height(22.0)
        .align_items(repose_core::AlignItems::CENTER))
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
        Text(label)
            .size(11.0)
            .color(th.primary),
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
    Column(Modifier::new().fill_max_width().padding_values(
        repose_core::PaddingValues { left: 0.0, right: 0.0, top: 2.0, bottom: 2.0 },
    )).child((
        Row(Modifier::new()
            .fill_max_width()
            .align_items(repose_core::AlignItems::CENTER))
        .child((
            Text(label).size(11.0).color(th.on_surface_variant),
            Box(Modifier::new().flex_grow(1.0)),
            Text(display_val).size(10.0).color(th.on_surface),
        )),
        material3::Slider(value, (min, max), step, on_change, Default::default())
            .modifier(Modifier::new().fill_max_width().height(20.0)),
    ))
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

    let clip_name = clip_name(clip);
    let start_fmt = fmt_us(clip.timeline_start.0);
    let end_fmt = fmt_us(clip.timeline_end().0);
    let dur_fmt = fmt_us(clip.timeline_duration.0);

    let mut children: Vec<View> = Vec::new();

    // Header info
    children.push(Text(&clip_name).size(12.0).color(th.on_surface));
    children.push(v_spacer(8.0));
    children.push(kv("Clip ID", &clip.id.0.to_string()[..8]));
    children.push(kv("Track", track_label));
    children.push(kv("Start", &start_fmt));
    children.push(kv("End", &end_fmt));
    children.push(kv("Duration", &dur_fmt));

    // Clip-type specific properties
    match &clip.kind {
        ClipKind::Video(v) => {
            children.push(section_header("Video"));
            children.push(kv("Source", &v.source_path));
            children.push(kv("Resolution", format!("{}x{}", v.width, v.height)));
            children.push(kv("FPS", format!("{:.1}", v.fps)));

            children.push(section_header("Properties"));

            // Speed
            children.push(slider_row(
                "Speed",
                clip.speed as f32,
                0.1, 4.0, Some(0.1),
                |v| format!("{:.1}x", v),
                {
                    let store = store.clone();
                    let clip_id = clip.id;
                    move |v| store.dispatch_edit(EditCommand::SetClipSpeed {
                        clip_id,
                        speed: v as f64,
                    })
                },
            ));

            // Volume
            children.push(slider_row(
                "Volume",
                clip.volume,
                0.0, 2.0, None,
                |v| format!("{}%", (v * 100.0).round() as i32),
                {
                    let store = store.clone();
                    let clip_id = clip.id;
                    move |v| store.dispatch_edit(EditCommand::SetClipVolume {
                        clip_id,
                        volume: v,
                    })
                },
            ));

            // Opacity
            children.push(slider_row(
                "Opacity",
                clip.opacity,
                0.0, 1.0, None,
                |v| format!("{}%", (v * 100.0).round() as i32),
                {
                    let store = store.clone();
                    let clip_id = clip.id;
                    move |v| store.dispatch_edit(EditCommand::SetClipOpacity {
                        clip_id,
                        opacity: v,
                    })
                },
            ));

            // Transition In
            children.push(section_header("Transitions"));
            children.push(transition_selector(
                store.clone(),
                clip.id,
                clip.transition_in.as_ref(),
                true,
            ));
            children.push(transition_selector(
                store.clone(),
                clip.id,
                clip.transition_out.as_ref(),
                false,
            ));

            // Filters
            children.push(section_header("Video Filters"));
            if v.filters.is_empty() {
                children.push(
                    Text("No filters").size(10.0).color(th.on_surface_variant.with_alpha(160)),
                );
            }
            for (idx, effect) in v.filters.iter().enumerate() {
                children.push(filter_row(
                    store.clone(),
                    clip.id,
                    idx,
                    effect,
                ));
            }
            children.push(add_filter_button(store.clone(), clip.id));

            // Masks
            if !v.masks.is_empty() {
                children.push(section_header("Masks"));
                children.push(
                    Text(format!("{} mask(s)", v.masks.len()))
                        .size(10.0).color(th.on_surface_variant),
                );
            }

            // Keyframes
            let kf_count = clip.keyframes.keyframes.len();
            if kf_count > 0 {
                children.push(section_header("Keyframes"));
                children.push(
                    Text(format!("{} keyframe(s)", kf_count))
                        .size(10.0).color(th.on_surface_variant),
                );
            }

            children.push(v_spacer(12.0));
        }
        ClipKind::Audio(a) => {
            children.push(section_header("Audio"));
            children.push(kv("Source", &a.source_path));

            children.push(section_header("Properties"));
            children.push(slider_row(
                "Volume",
                clip.volume,
                0.0, 2.0, None,
                |v| format!("{}%", (v * 100.0).round() as i32),
                {
                    let store = store.clone();
                    let clip_id = clip.id;
                    move |v| store.dispatch_edit(EditCommand::SetClipVolume {
                        clip_id,
                        volume: v,
                    })
                },
            ));

            children.push(section_header("Audio Filters"));
            if a.filters.is_empty() {
                children.push(
                    Text("No filters").size(10.0).color(th.on_surface_variant.with_alpha(160)),
                );
            }
            for (idx, filter) in a.filters.iter().enumerate() {
                children.push(audio_filter_row(store.clone(), clip.id, idx, filter));
            }

            children.push(v_spacer(12.0));
        }
        ClipKind::Text(t) => {
            children.push(section_header("Text"));
            let store_clone = store.clone();
            let clip_id = clip.id;
            let text_content = t.text.clone();

            const TEXT_KEY: u64 = 0x54455854u64;
            let text_state = get_textfield_state(TEXT_KEY).unwrap_or_else(|| {
                let s = Rc::new(std::cell::RefCell::new(TextFieldState::new()));
                s.borrow_mut().text = text_content.clone();
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
                            let store = store_clone.clone();
                            let clip_id = clip_id;
                            move |v| {
                                store.dispatch_edit(EditCommand::UpdateTextContent {
                                    clip_id,
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
            children.push(kv("Position", format!("({:.2}, {:.2})", t.style.position_x, t.style.position_y)));

            children.push(section_header("Transitions"));
            children.push(transition_selector(
                store.clone(),
                clip.id,
                clip.transition_in.as_ref(),
                true,
            ));
            children.push(transition_selector(
                store.clone(),
                clip.id,
                clip.transition_out.as_ref(),
                false,
            ));

            children.push(v_spacer(12.0));
        }
        ClipKind::Subtitle(s) => {
            children.push(section_header("Subtitle"));
            children.push(kv("Source", &s.source_path));
            if let Some(fp) = &s.font_path {
                children.push(kv("Font", fp));
            }

            children.push(section_header("Properties"));
            children.push(slider_row(
                "Opacity",
                clip.opacity,
                0.0, 1.0, None,
                |v| format!("{}%", (v * 100.0).round() as i32),
                {
                    let store = store.clone();
                    let clip_id = clip.id;
                    move |v| store.dispatch_edit(EditCommand::SetClipOpacity {
                        clip_id,
                        opacity: v,
                    })
                },
            ));

            children.push(section_header("Transitions"));
            children.push(transition_selector(
                store.clone(),
                clip.id,
                clip.transition_in.as_ref(),
                true,
            ));
            children.push(transition_selector(
                store.clone(),
                clip.id,
                clip.transition_out.as_ref(),
                false,
            ));

            children.push(v_spacer(12.0));
        }
        _ => {}
    }

    ScrollArea(
        Modifier::new().fill_max_size(),
        remember_scroll_state("inspector_clip"),
        Column(Modifier::new().fill_max_width().padding(10.0)).child(children),
    )
}

fn clip_name(clip: &Clip) -> String {
    match &clip.kind {
        ClipKind::Video(v) => {
            let name = v.source_path.split('/').last()
                .or_else(|| v.source_path.split('\\').last())
                .unwrap_or("Video");
            name.to_string()
        }
        ClipKind::Audio(a) => {
            let name = a.source_path.split('/').last()
                .or_else(|| a.source_path.split('\\').last())
                .unwrap_or("Audio");
            name.to_string()
        }
        ClipKind::Text(t) => {
            if t.text.len() > 30 {
                format!("{}…", &t.text[..30])
            } else {
                t.text.clone()
            }
        }
        ClipKind::Subtitle(s) => {
            let name = s.source_path.split('/').last()
                .or_else(|| s.source_path.split('\\').last())
                .unwrap_or("Subtitle");
            name.to_string()
        }
        _ => "Clip".to_string(),
    }
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

fn transition_selector(
    store: Rc<Store>,
    clip_id: ClipId,
    transition: Option<&Transition>,
    is_in: bool,
) -> View {
    let th = theme();
    let label = if is_in { "Transition In" } else { "Transition Out" };

    let status = match transition {
        Some(t) => format!("{:?} ({}ms)", t.kind, t.duration.as_micros() / 1000),
        None => "None".to_string(),
    };

    let store_for_set = store.clone();
    let store_for_clear = store.clone();

    Row(Modifier::new()
        .fill_max_width()
        .height(28.0)
        .align_items(repose_core::AlignItems::CENTER))
    .child(vec![
        Text(label).size(10.0).color(th.on_surface_variant),
        Box(Modifier::new().flex_grow(1.0)),
        Text(status).size(10.0).color(th.on_surface),
        h_spacer(4.0),
        if transition.is_none() {
            material3::TextButton(
                Modifier::new().height(22.0),
                move || {
                    store_for_set.dispatch_edit(if is_in {
                        EditCommand::SetTransitionIn {
                            clip_id,
                            transition: Some(Transition::new(
                                TransitionKind::CrossFade,
                                MediaDuration::from_micros(500_000),
                            )),
                        }
                    } else {
                        EditCommand::SetTransitionOut {
                            clip_id,
                            transition: Some(Transition::new(
                                TransitionKind::CrossFade,
                                MediaDuration::from_micros(500_000),
                            )),
                        }
                    });
                },
                Default::default(),
                || Text("+").size(12.0),
            )
        } else {
            material3::TextButton(
                Modifier::new().height(22.0),
                move || {
                    store_for_clear.dispatch_edit(if is_in {
                        EditCommand::SetTransitionIn {
                            clip_id,
                            transition: None,
                        }
                    } else {
                        EditCommand::SetTransitionOut {
                            clip_id,
                            transition: None,
                        }
                    });
                },
                Default::default(),
                || Text("×").size(12.0),
            )
        },
    ])
}

fn filter_row(
    store: Rc<Store>,
    clip_id: ClipId,
    idx: usize,
    effect: &VideoEffect,
) -> View {
    let th = theme();
    let name = filter_name(&effect.filter);
    let enabled = effect.enabled;
    let fg = if enabled { th.on_surface } else { th.on_surface_variant.with_alpha(160) };

    let store_for_toggle = store.clone();
    let store_for_move_up = store.clone();
    let store_for_move_down = store.clone();
    let store_for_remove = store.clone();

    Row(Modifier::new()
        .fill_max_width()
        .height(26.0)
        .align_items(repose_core::AlignItems::CENTER))
    .child(vec![
        Box(Modifier::new()
            .size(14.0, 14.0)
            .background(if enabled { th.primary } else { th.surface_variant })
            .clip_rounded(2.0)
            .clickable()
            .on_pointer_down(move |_| {
                store_for_toggle.dispatch_edit(
                    EditCommand::SetVideoFilterEnabled {
                        clip_id,
                        index: idx,
                        enabled: !enabled,
                    },
                );
            })),
        h_spacer(6.0),
        Text(name).size(10.0).color(fg).modifier(Modifier::new().flex_grow(1.0)),
        material3::IconButton(
            Icon(Icons::arrow_upward).size(12.0),
            move || {
                if idx > 0 {
                    store_for_move_up.dispatch_edit(
                        EditCommand::MoveVideoFilter {
                            clip_id,
                            from: idx,
                            to: idx - 1,
                        },
                    );
                }
            },
            Default::default(),
        ),
        material3::IconButton(
            Icon(Icons::arrow_downward).size(12.0),
            move || {
                store_for_move_down.dispatch_edit(
                    EditCommand::MoveVideoFilter {
                        clip_id,
                        from: idx,
                        to: idx + 1,
                    },
                );
            },
            Default::default(),
        ),
        material3::IconButton(
            Icon(Icons::close).size(12.0),
            move || {
                store_for_remove.dispatch_edit(
                    EditCommand::RemoveVideoFilter {
                        clip_id,
                        index: idx,
                    },
                );
            },
            Default::default(),
        ),
    ])
}

fn filter_name(filter: &miniter_domain::VideoFilter) -> String {
    match filter {
        miniter_domain::VideoFilter::Brightness { .. } => "Brightness".into(),
        miniter_domain::VideoFilter::Contrast { .. } => "Contrast".into(),
        miniter_domain::VideoFilter::Saturation { .. } => "Saturation".into(),
        miniter_domain::VideoFilter::Grayscale => "Grayscale".into(),
        miniter_domain::VideoFilter::Blur { .. } => "Blur".into(),
        miniter_domain::VideoFilter::Sharpen { .. } => "Sharpen".into(),
        miniter_domain::VideoFilter::Sepia => "Sepia".into(),
        miniter_domain::VideoFilter::Hue { .. } => "Hue".into(),
        miniter_domain::VideoFilter::Crop { .. } => "Crop".into(),
        miniter_domain::VideoFilter::Rotate { .. } => "Rotate".into(),
        miniter_domain::VideoFilter::Flip { .. } => "Flip".into(),
        miniter_domain::VideoFilter::Transform { .. } => "Transform".into(),
        miniter_domain::VideoFilter::Speed { .. } => "Speed".into(),
        miniter_domain::VideoFilter::Opacity { .. } => "Opacity".into(),
        miniter_domain::VideoFilter::BlendMode { .. } => "Blend".into(),
        _ => "Unknown".into(),
    }
}

fn add_filter_button(store: Rc<Store>, clip_id: ClipId) -> View {
    let th = theme();
    material3::TextButton(
        Modifier::new().height(28.0),
        {
            let store = store.clone();
            move || {
                store.dispatch_edit(EditCommand::AddVideoFilter {
                    clip_id,
                    filter: VideoEffect::new(
                        miniter_domain::VideoFilter::Brightness { value: 0.0 },
                    ),
                });
            }
        },
        Default::default(),
        || {
            Row(Modifier::new().align_items(repose_core::AlignItems::CENTER)).child((
                Icon(Icons::add).size(12.0),
                h_spacer(4.0),
                Text("Add Filter").size(10.0),
            ))
        },
    )
}

fn audio_filter_row(
    store: Rc<Store>,
    clip_id: ClipId,
    idx: usize,
    filter: &miniter_domain::AudioFilter,
) -> View {
    let th = theme();
    let name = match filter {
        miniter_domain::AudioFilter::Volume { .. } => "Volume",
        miniter_domain::AudioFilter::FadeIn { .. } => "Fade In",
        miniter_domain::AudioFilter::FadeOut { .. } => "Fade Out",
        miniter_domain::AudioFilter::Normalize => "Normalize",
        _ => "Unknown",
    };
    let store_for_remove = store.clone();

    Row(Modifier::new()
        .fill_max_width()
        .height(24.0)
        .align_items(repose_core::AlignItems::CENTER))
    .child(vec![
        Text(name).size(10.0).color(th.on_surface).modifier(Modifier::new().flex_grow(1.0)),
        material3::IconButton(
            Icon(Icons::close).size(12.0),
            move || {
                store_for_remove.dispatch_edit(
                    EditCommand::RemoveAudioFilter {
                        clip_id,
                        index: idx,
                    },
                );
            },
            Default::default(),
        ),
    ])
}

fn asset_inspector(asset: &snapshort_usecases::Asset) -> View {
    let th = theme();
    let path = asset.path.to_string_lossy().to_string();
    let status = format!("{:?}", asset.status);
    let dur_us = asset.media_info.as_ref().map(|m| m.duration_ms as i64 * 1000).unwrap_or(0);

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
