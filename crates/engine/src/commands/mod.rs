//! The command registry.
//!
//! Every user-visible action has a stable id and JSON parameters. Menus,
//! shortcuts, panels and timeline gestures all map to one id, and the same id
//! drives the UI, the CLI, the control channel and the MCP server.

use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;

use serde_json::Value;

use crate::{EngineError, Result, Session};

mod assets;
mod audio;
mod edit;
mod export;
mod markers;
mod mixer;
mod playback;

mod presets;
mod project;
mod render;
mod scopes;
mod text;
mod timeline;
mod transcript;

pub type RunFuture<'a> = Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>>;
pub type EnabledFuture<'a> =
    Pin<Box<dyn Future<Output = std::result::Result<(), String>> + Send + 'a>>;

/// Metadata plus the implementation of one command.
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Menu placement, e.g. `["File", "New"]`. Empty when not in a menu.
    pub menu: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    /// Parameter description (JSON-ish), for agents and `command_list`.
    pub params: &'static str,
    /// Why the command cannot run right now (`Ok` when it can).
    pub enabled: for<'a> fn(&'a Session) -> EnabledFuture<'a>,
    pub run: for<'a> fn(&'a Session, &'a Value) -> RunFuture<'a>,
}

macro_rules! cmd {
    ($id:literal, $label:literal, [$($m:literal),*], $sc:expr, $params:literal, $en:expr, $run:expr) => {
        $crate::commands::CommandSpec {
            id: $id,
            label: $label,
            menu: &[$($m),*],
            shortcut: $sc,
            params: $params,
            enabled: $en,
            run: $run,
        }
    };
}

macro_rules! query {
    ($id:literal, $label:literal, $params:literal, $run:expr) => {
        cmd!(
            $id,
            $label,
            [],
            None::<&'static str>,
            $params,
            $crate::commands::always,
            $run
        )
    };
}

pub(crate) use {cmd, query};

pub fn command_specs() -> &'static [CommandSpec] {
    static SPECS: OnceLock<Vec<CommandSpec>> = OnceLock::new();
    SPECS.get_or_init(build)
}

fn build() -> Vec<CommandSpec> {
    let mut all = Vec::new();
    all.extend(project::specs());
    all.extend(assets::specs());
    all.extend(mixer::specs());
    all.extend(audio::specs());
    all.extend(transcript::specs());
    all.extend(markers::specs());
    all.extend(timeline::specs());
    all.extend(edit::specs());
    all.extend(export::specs());
    all.extend(presets::specs());
    all.extend(playback::specs());
    all.extend(render::specs());
    all.extend(scopes::specs());
    all.extend(text::specs());
    all.sort_by(|a, b| a.id.cmp(b.id));
    all
}

pub fn find(id: &str) -> Option<&'static CommandSpec> {
    command_specs().iter().find(|c| c.id == id)
}

pub fn spec_json(spec: &CommandSpec, session: &Session) -> Value {
    let enabled = futures_block(spec.enabled, session);
    serde_json::json!({
        "id": spec.id,
        "label": spec.label,
        "menu": spec.menu,
        "shortcut": spec.shortcut,
        "params": spec.params,
        "enabled": enabled.is_ok(),
        "disabledReason": enabled.err(),
    })
}

/// Run an enablement check without a runtime: the checks only read state that
/// is already unlocked, so they never block.
fn futures_block(
    check: for<'a> fn(&'a Session) -> EnabledFuture<'a>,
    session: &Session,
) -> std::result::Result<(), String> {
    let mut future = check(session);
    // The future is polled once; every body completes without awaiting a lock
    // that could be held (try_read), so one poll suffices.
    let waker = std::task::Waker::noop();
    let mut cx = std::task::Context::from_waker(waker);
    match future.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => Ok(()),
    }
}

// ---------- enablement ----------

pub(crate) fn always(_s: &Session) -> EnabledFuture<'_> {
    Box::pin(async { Ok(()) })
}

fn has_project(s: &Session) -> EnabledFuture<'_> {
    Box::pin(async move {
        match s.services().project.current_project().await {
            Some(_) => Ok(()),
            None => Err("no project is open".into()),
        }
    })
}

fn can_undo(s: &Session) -> EnabledFuture<'_> {
    Box::pin(async move {
        let guard = s.services().project.editor().await;
        let guard = guard.read().await;
        match guard.as_ref() {
            Some(e) if e.history.can_undo() => Ok(()),
            _ => Err("nothing to undo".into()),
        }
    })
}

fn can_redo(s: &Session) -> EnabledFuture<'_> {
    Box::pin(async move {
        let guard = s.services().project.editor().await;
        let guard = guard.read().await;
        match guard.as_ref() {
            Some(e) if e.history.can_redo() => Ok(()),
            _ => Err("nothing to redo".into()),
        }
    })
}

fn has_selection(s: &Session) -> EnabledFuture<'_> {
    Box::pin(async move {
        let selection = s.selection();
        if selection.clips.is_empty() && selection.tracks.is_empty() {
            Err("nothing selected".into())
        } else {
            Ok(())
        }
    })
}

fn export_running(s: &Session) -> EnabledFuture<'_> {
    Box::pin(async move {
        if s.export_in_flight() {
            Ok(())
        } else {
            Err("no export is running".into())
        }
    })
}

/// Resolve the clips a command acts on: the ids named in `params`, or the
/// session selection when it named none.
pub(crate) async fn target_clips(
    s: &Session,
    p: &Value,
    cmd: &str,
) -> Result<Vec<miniter_domain::ClipId>> {
    let named = crate::params::clip_ids_p(p);
    if !named.is_empty() {
        return Ok(named);
    }
    let selection = s.selection().clips.clone();
    if selection.is_empty() {
        return Err(EngineError::bad(
            cmd,
            "name `clips` (or select something first)",
        ));
    }
    Ok(selection)
}

/// The edit command behind `id`, for `timeline.batch`. `None` when the
/// command is not a plain edit (it resolves tracks, creates clips or reaches
/// into assets).
pub(crate) async fn edit_command_of(
    s: &Session,
    id: &str,
    p: &Value,
) -> Result<Option<miniter_usecases::EditCommand>> {
    Ok(timeline::edit_of(s, id, p).await?.map(|e| e.command))
}

/// Apply one edit command as its own undo step and return the timeline.
pub(crate) async fn apply_edit(
    s: &Session,
    cmd: miniter_usecases::EditCommand,
    label: &str,
) -> Result<Value> {
    s.services()
        .project
        .dispatch_timeline_command(cmd, label.to_string())
        .await?;
    Ok(crate::inspect::timeline_json(s).await)
}

/// Apply many edit commands as one undo step.
pub(crate) async fn apply_batch(
    s: &Session,
    cmds: Vec<miniter_usecases::EditCommand>,
    label: &str,
) -> Result<Value> {
    s.services()
        .project
        .dispatch_timeline_command(
            miniter_usecases::EditCommand::Batch {
                label: label.to_string(),
                commands: cmds,
            },
            label.to_string(),
        )
        .await?;
    Ok(crate::inspect::timeline_json(s).await)
}
