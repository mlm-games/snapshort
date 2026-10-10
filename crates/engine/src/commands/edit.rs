//! Editing commands: undo/redo, selection, batched edits.

use serde_json::{Value, json};

use crate::commands::cmd;
use crate::params::{self, ids_p, require_str};
use crate::{EngineError, Session};

use super::{RunFuture, apply_batch, can_redo, can_undo, has_project, has_selection, query};

pub fn specs() -> Vec<crate::commands::CommandSpec> {
    vec![
        cmd!(
            "app.undo",
            "Undo",
            ["Edit", "Undo"],
            Some("Cmd+Z"),
            "{}",
            can_undo,
            run_undo
        ),
        cmd!(
            "app.redo",
            "Redo",
            ["Edit", "Redo"],
            Some("Cmd+Shift+Z"),
            "{}",
            can_redo,
            run_redo
        ),
        cmd!(
            "selection.select",
            "Select",
            [],
            None::<&'static str>,
            "{\"clips\"?: [id], \"tracks\"?: [id], \"clear\"?: bool}",
            has_project,
            run_select
        ),
        cmd!(
            "selection.clear",
            "Deselect",
            [],
            None::<&'static str>,
            "{}",
            has_selection,
            run_clear
        ),
        cmd!(
            "timeline.batch",
            "Run Several Edits",
            [],
            None::<&'static str>,
            "{\"label\"?: string, \"commands\": [{\"id\": command-id, \"params\": object}]}",
            has_project,
            run_batch
        ),
        query!("app.history", "History", "{}", run_history),
        query!("app.inspect", "Editor State", "{}", run_inspect),
        query!("app.settings", "Editor Settings", "{}", run_settings),
        query!("selection.inspect", "Selection", "{}", run_selection),
    ]
}

fn run_undo<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        s.dispatch(crate::SessionCommand::Undo).await?;
        Ok(crate::inspect::undo_json(s).await)
    })
}

fn run_redo<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        s.dispatch(crate::SessionCommand::Redo).await?;
        Ok(crate::inspect::undo_json(s).await)
    })
}

fn run_select<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let clear = params::bool_p(p, "clear").unwrap_or(false);
        let clips = ids_p(p, "clips", "clip");
        let tracks = ids_p(p, "tracks", "track");
        if clear {
            s.selection().clear();
        }
        {
            let mut selection = s.selection();
            for id in clips {
                selection.select_clip(miniter_domain::ClipId(id));
            }
            for id in tracks {
                selection.select_track(miniter_domain::TrackId(id));
            }
        }
        Ok(crate::inspect::selection_json(s).await)
    })
}

fn run_clear<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        s.selection().clear();
        Ok(crate::inspect::selection_json(s).await)
    })
}

fn run_history<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move { Ok(crate::inspect::undo_json(s).await) })
}

fn run_selection<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move { Ok(crate::inspect::selection_json(s).await) })
}

fn run_inspect<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move { Ok(crate::inspect::document_json(s).await) })
}

fn run_settings<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move { Ok(crate::inspect::settings_json(s).await) })
}

/// Run several commands as one undo step. Each step is an engine command id
/// with its own params, exactly as `command_run` takes them.
fn run_batch<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let steps = p
            .get("commands")
            .and_then(Value::as_array)
            .ok_or_else(|| EngineError::bad("timeline.batch", "`commands` is required"))?;
        let label = require_str(p, "label", "timeline.batch")
            .unwrap_or("Batch")
            .to_string();

        let mut collected: Vec<miniter_usecases::EditCommand> = Vec::new();
        for step in steps {
            let id = step
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| EngineError::bad("timeline.batch", "each step needs an `id`"))?;
            let params = step.get("params").cloned().unwrap_or_else(|| json!({}));
            let Some(command) = crate::commands::edit_command_of(s, id, &params).await? else {
                return Err(EngineError::bad(
                    "timeline.batch",
                    format!("`{id}` is not an edit command"),
                ));
            };
            collected.push(command);
        }
        apply_batch(s, collected, &label).await
    })
}
