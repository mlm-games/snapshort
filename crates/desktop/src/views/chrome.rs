#![allow(non_snake_case)]
//! Shared Material 3 chrome.
//!
//! All panel chrome goes through these helpers so the NLE keeps one design
//! system path: theme tokens -> M3 components -> shared chrome. No hardcoded
//! font sizes or parallel color modules in app chrome. View constructors use
//! UpperCamelCase per M3/Compose convention (mirrors renamite-ui).

use repose_core::{AlignItems, Dp, Modifier, PaddingValues, Sp, View, remember_with_key, theme};
use repose_material::Icon;
use repose_material::Symbol;
use repose_material::material3::{
    FilledTonalIconButton, IconButton, IconButtonConfig, Surface, SurfaceConfig, TooltipBox,
    TooltipConfig, TooltipState,
};
use repose_ui::{Box, Column, Row, Text, TextStyle, ViewExt};

/// Rounded M3 surface every dock panel body sits on.
pub fn PanelSurface(content: View) -> View {
    Surface(
        SurfaceConfig {
            modifier: Modifier::new().fill_max_size(),
            color: theme().surface_container_low,
            content_color: theme().on_surface,
            shape_radius: Dp(12.0),
            border: Some((Dp(1.0), theme().outline_variant.with_alpha(140))),
            ..Default::default()
        },
        move || content,
    )
}

/// 44dp panel header: leading icon + title_small + trailing actions.
pub fn PanelHeader(icon: Symbol, title: impl Into<String>, actions: Vec<View>) -> View {
    let title = title.into();
    Row(Modifier::new()
        .height(Dp(44.0))
        .fill_max_width()
        .padding_values(PaddingValues {
            left: Dp(12.0),
            right: Dp(8.0),
            top: Dp(0.0),
            bottom: Dp(0.0),
        })
        .align_items(AlignItems::CENTER)
        .gap(Dp(8.0)))
    .child((
        Icon(icon).size(Sp(20.0)).color(theme().primary),
        Text(title)
            .size(theme().typography.title_small)
            .color(theme().on_surface),
        Box(Modifier::new().flex_grow(1.0)),
        Row(Modifier::new().gap(Dp(2.0)).align_items(AlignItems::CENTER)).child(actions),
    ))
}

/// 40dp icon tool with hover tooltip. Selected state renders tonal-filled.
pub fn ToolIcon(
    key: impl Into<String>,
    symbol: Symbol,
    tooltip: impl Into<String>,
    selected: bool,
    enabled: bool,
    on_click: impl Fn() + 'static,
) -> View {
    let tip = remember_with_key(key.into(), TooltipState::new);
    let cfg = IconButtonConfig {
        enabled,
        container_size: Some(Dp(40.0)),
        shape_radius: Some(Dp(12.0)),
        ..Default::default()
    };
    let btn = if selected {
        FilledTonalIconButton(Icon(symbol).size(Sp(22.0)), on_click, cfg)
    } else {
        IconButton(Icon(symbol).size(Sp(22.0)), on_click, cfg)
    };
    TooltipBox(
        tooltip,
        tip.clone(),
        Modifier::new(),
        btn,
        TooltipConfig::default(),
    )
}

/// Small rounded status pill (project dirty state, counts).
pub fn StatusChip(label: impl Into<String>, emphasis: bool) -> View {
    let th = theme();
    let (bg, fg) = if emphasis {
        (th.primary_container, th.on_primary_container)
    } else {
        (th.surface_container_highest, th.on_surface_variant)
    };
    Text(label.into())
        .size(th.typography.label_small)
        .color(fg)
        .modifier(
            Modifier::new()
                .padding_values(PaddingValues {
                    left: Dp(10.0),
                    right: Dp(10.0),
                    top: Dp(4.0),
                    bottom: Dp(4.0),
                })
                .background(bg)
                .clip_rounded(Dp(999.0)),
        )
}

/// Honest empty state: centered icon + title + body. Used instead of fake
/// interactive content for panels that aren't wired yet.
pub fn EmptyState(icon: Symbol, title: &str, body: &str) -> View {
    let th = theme();
    Column(
        Modifier::new()
            .fill_max_size()
            .padding(Dp(24.0))
            .align_items(AlignItems::CENTER)
            .justify_content(repose_core::AlignContent::CENTER)
            .gap(Dp(8.0)),
    )
    .child((
        Icon(icon).size(Sp(36.0)).color(th.on_surface_variant),
        Text(title.to_owned())
            .size(th.typography.title_medium)
            .color(th.on_surface),
        Text(body.to_owned())
            .size(th.typography.body_medium)
            .color(th.on_surface_variant)
            .max_lines(4)
            .text_align(repose_core::text::TextAlign::Center),
    ))
}

/// Vertical divider for tool rows (1dp wide, 18dp tall).
pub fn HRule() -> View {
    Box(Modifier::new()
        .width(Dp(1.0))
        .height(Dp(18.0))
        .background(theme().outline_variant))
}

pub fn VSpace(h: f32) -> View {
    Box(Modifier::new().height(Dp(h)))
}

pub fn HSpace(w: f32) -> View {
    Box(Modifier::new().width(Dp(w)))
}
