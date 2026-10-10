//! Project commands: files, recovery, export profile.

use serde_json::{Value, json};
use snapshort_usecases::{ProjectCommand, TimelineMarkerData};

use crate::commands::cmd;
use crate::params::{self, ok, str_p, value_p};
use crate::{EngineError, Result, Session};

use super::{has_project, query};

pub fn specs() -> Vec<crate::commands::CommandSpec> {
    vec![
        cmd!(
            "project.new",
            "New Project",
            ["File", "New"],
            Some("Cmd+N"),
            "{\"name\"?: string}",
            always_new,
            run_new
        ),
        cmd!(
            "project.open",
            "Open Project",
            ["File", "Open"],
            Some("Cmd+O"),
            "{\"path\": string}",
            always_new,
            run_open
        ),
        cmd!(
            "project.save",
            "Save Project",
            ["File", "Save"],
            Some("Cmd+S"),
            "{\"markers\"?: [{\"timestampUs\":int,\"label\":string}]}",
            has_project,
            run_save
        ),
        cmd!(
            "project.saveAs",
            "Save Project As",
            ["File", "Save As"],
            Some("Cmd+Shift+S"),
            "{\"path\": string, \"markers\"?: [{\"timestampUs\":int,\"label\":string}]}",
            has_project,
            run_save_as
        ),
        cmd!(
            "project.close",
            "Close Project",
            ["File", "Close"],
            None::<&'static str>,
            "{}",
            has_project,
            run_close
        ),
        cmd!(
            "project.recover",
            "Restore Unsaved Changes",
            [],
            None::<&'static str>,
            "{}",
            always_new,
            run_recover
        ),
        cmd!(
            "project.discardRecovery",
            "Discard Unsaved Changes",
            [],
            None::<&'static str>,
            "{}",
            always_new,
            run_discard_recovery
        ),
        cmd!(
            "project.setExportProfile",
            "Set Export Profile",
            [],
            None::<&'static str>,
            "{\"profile\": ExportProfile}",
            has_project,
            run_set_export_profile
        ),
        query!("project.inspect", "Project", "{}", run_inspect),
        query!("project.list", "Stored Projects", "{}", run_list),
        query!(
            "project.recovery",
            "Recovery Versions",
            "{}",
            run_recovery_versions
        ),
        cmd!(
            "project.restoreVersion",
            "Restore an Autosave Version",
            [],
            None::<&str>,
            "{\"index\"?: int}",
            always_new,
            run_restore_version
        ),
    ]
}

fn always_new(_s: &Session) -> super::EnabledFuture<'_> {
    Box::pin(async { Ok(()) })
}

fn run_new<'a>(s: &'a Session, p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let name = str_p(p, "name").unwrap_or("Untitled").to_string();
        s.dispatch(crate::SessionCommand::Project(ProjectCommand::Create {
            name,
        }))
        .await?;
        Ok(crate::inspect::project_json(s).await)
    })
}

fn run_open<'a>(s: &'a Session, p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let path = require_path(p, "project.open")?;
        s.dispatch(crate::SessionCommand::Project(ProjectCommand::Open {
            path,
        }))
        .await?;
        s.set_timeline_fps_from_clips(
            &s.services()
                .project
                .current_timeline()
                .await
                .ok_or(EngineError::NoProject)?,
        );
        Ok(crate::inspect::project_json(s).await)
    })
}

fn run_save<'a>(s: &'a Session, p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let markers = markers_p(p, "project.save")?;
        s.dispatch(crate::SessionCommand::Project(ProjectCommand::Save {
            markers,
        }))
        .await?;
        Ok(json!({
            "ok": true,
            "path": s.services().project.current_path().await.map(|p| p.display().to_string()),
        }))
    })
}

fn run_save_as<'a>(s: &'a Session, p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let path = require_path(p, "project.saveAs")?;
        let markers = markers_p(p, "project.saveAs")?;
        s.dispatch(crate::SessionCommand::Project(ProjectCommand::SaveAs {
            path,
            markers,
        }))
        .await?;
        Ok(json!({
            "ok": true,
            "path": s.services().project.current_path().await.map(|p| p.display().to_string()),
        }))
    })
}

fn run_close<'a>(s: &'a Session, _p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        s.dispatch(crate::SessionCommand::Project(ProjectCommand::Close))
            .await?;
        Ok(ok())
    })
}

fn run_recover<'a>(s: &'a Session, _p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        s.dispatch(crate::SessionCommand::Project(
            ProjectCommand::RestoreAutosave,
        ))
        .await?;
        Ok(crate::inspect::project_json(s).await)
    })
}

fn run_discard_recovery<'a>(s: &'a Session, _p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        s.dispatch(crate::SessionCommand::Project(
            ProjectCommand::DiscardAutosave,
        ))
        .await?;
        Ok(ok())
    })
}

fn run_set_export_profile<'a>(s: &'a Session, p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let profile = params::require_object::<miniter_domain::ExportProfile>(
            p,
            "profile",
            "project.setExportProfile",
        )?;
        crate::commands::apply_edit(
            s,
            miniter_usecases::EditCommand::SetExportProfile { profile },
            "Set Export Profile",
        )
        .await
    })
}

fn run_inspect<'a>(s: &'a Session, _p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move { Ok(crate::inspect::project_json(s).await) })
}

fn run_recovery_versions<'a>(s: &'a Session, _p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let versions = s.autosave_versions();
        Ok(serde_json::json!({"versions": versions, "count": versions.len()}))
    })
}

fn run_restore_version<'a>(s: &'a Session, p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let index = params::usize_p(p, "index");
        s.restore_autosave_version(index).await?;
        Ok(crate::inspect::project_json(s).await)
    })
}

fn run_list<'a>(s: &'a Session, _p: &'a Value) -> super::RunFuture<'a> {
    Box::pin(async move {
        let projects = s.services().project.list_projects().await?;
        let items: Vec<Value> = projects
            .iter()
            .map(|p| {
                json!({
                    "id": p.id.0.to_string(),
                    "name": p.meta.name,
                    "tracks": p.timeline.tracks.len(),
                    "clips": p.timeline.tracks.iter().map(|t| t.clips.len()).sum::<usize>(),
                    "modifiedAt": p.meta.modified_at,
                })
            })
            .collect();
        Ok(json!({ "projects": items }))
    })
}

fn markers_p(p: &Value, cmd: &str) -> Result<Vec<TimelineMarkerData>> {
    match value_p(p, "markers") {
        Some(v) => {
            serde_json::from_value(v).map_err(|e| EngineError::bad(cmd, format!("`markers`: {e}")))
        }
        None => Ok(Vec::new()),
    }
}

pub(crate) fn require_path(p: &Value, cmd: &str) -> Result<std::path::PathBuf> {
    params::require_file(p, "path", cmd)
}
