//! Title templates: named text styles, so a title is one word, not a style sheet.

use miniter_domain::{TextAlignment, TextStyle};
use serde_json::{Value, json};

use crate::commands::cmd;
use crate::params;
use crate::{EngineError, Result, Session};

use super::{RunFuture, has_project, query};

/// One named style.
pub struct Template {
    pub name: &'static str,
    pub description: &'static str,
    pub style: TextStyle,
}

fn style(font_size: f32, color: &str, alignment: TextAlignment, bold: bool) -> TextStyle {
    TextStyle {
        font_family: "sans-serif".into(),
        font_size,
        color: color.into(),
        background_color: None,
        alignment,
        position_x: 0.5,
        position_y: 0.5,
        outline_color: None,
        outline_width: 0.0,
        shadow: false,
        bold,
        italic: false,
    }
}

/// The templates that ship with the app.
pub fn templates() -> Vec<Template> {
    vec![
        Template {
            name: "title",
            description: "Large centred title",
            style: style(72.0, "FFFFFFFF", TextAlignment::Center, true),
        },
        Template {
            name: "subtitle",
            description: "Centred subtitle",
            style: style(40.0, "FFFFFFFF", TextAlignment::Center, false),
        },
        Template {
            name: "lower-third",
            description: "Small left-aligned graphic line",
            style: style(28.0, "FFE8EAED", TextAlignment::Left, false),
        },
        Template {
            name: "caption",
            description: "Small bottom-centred caption",
            style: style(24.0, "FFD0D4DA", TextAlignment::Center, false),
        },
    ]
}

pub fn find_template(name: &str) -> Option<Template> {
    templates().into_iter().find(|t| t.name == name)
}

fn template_json(template: &Template) -> Value {
    json!({
        "name": template.name,
        "description": template.description,
        "style": template.style,
    })
}

pub fn specs() -> Vec<crate::commands::CommandSpec> {
    vec![
        query!("text.templates", "Title Templates", "{}", run_list),
        cmd!(
            "text.style",
            "Style a Clip's Text",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"template\": string}",
            has_project,
            run_style
        ),
    ]
}

fn run_list<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let items: Vec<Value> = templates().iter().map(template_json).collect();
        Ok(json!({"templates": items, "count": items.len()}))
    })
}

/// Apply a named template's style to a text clip.
fn run_style<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let name = params::require_str(p, "template", "text.style")?;
        let Some(template) = find_template(name) else {
            return Err(EngineError::bad(
                "text.style",
                format!("no template `{name}`"),
            ));
        };
        let clip_id = params::require_clip(p, "clip", "text.style")?;
        crate::commands::apply_edit(
            s,
            miniter_usecases::EditCommand::UpdateTextStyle {
                clip_id,
                style: template.style.clone(),
            },
            "Text Style",
        )
        .await
    })
}
