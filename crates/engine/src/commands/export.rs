//! Interchange commands: write a cut out for another tool, bring captions in.

use std::path::PathBuf;

use serde_json::{Value, json};
use snapshort_interchange::{self, Rate};

use crate::commands::cmd;
use crate::params::{self, require_str};
use crate::{EngineError, Session};

use super::{RunFuture, has_project, query};

pub fn specs() -> Vec<crate::commands::CommandSpec> {
    vec![
        cmd!(
            "export.fcpxml",
            "Export FCPXML",
            ["File", "Export", "FCPXML"],
            None::<&'static str>,
            "{\"path\": string}",
            has_project,
            run_fcpxml
        ),
        cmd!(
            "export.edl",
            "Export EDL",
            ["File", "Export", "EDL"],
            None::<&'static str>,
            "{\"path\": string}",
            has_project,
            run_edl
        ),
        cmd!(
            "export.srt",
            "Export Captions",
            ["File", "Export", "SRT"],
            None::<&'static str>,
            "{\"path\": string}",
            has_project,
            run_srt
        ),
        cmd!(
            "captions.import",
            "Import Captions",
            ["File", "Import", "Captions"],
            None::<&'static str>,
            "{\"path\": string, \"track\"?: id, \"start\"?: time}",
            has_project,
            run_import_captions
        ),
        query!("export.formats", "Export Formats", "{}", run_formats),
    ]
}

/// The sequence frame rate: the first video clip's, else the session's.
fn rate_of(session: &Session, timeline: &miniter_domain::Timeline) -> Rate {
    let fps = timeline
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find_map(|c| match &c.kind {
            miniter_domain::ClipKind::Video(v) if v.fps > 0.0 => Some(v.fps),
            _ => None,
        })
        .unwrap_or(session.fps() as f64);
    Rate::from_f64(fps)
}

/// An asset's file stem, for clip names in exported documents.
fn source_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("clip")
        .to_string()
}

fn output_path(p: &Value, cmd: &str) -> Result<PathBuf, EngineError> {
    params::require_file(p, "path", cmd)
}

fn write_out(path: &std::path::Path, body: &str, cmd: &str) -> Result<Value, EngineError> {
    std::fs::write(path, body)
        .map_err(|e| EngineError::bad(cmd, format!("cannot write {}: {e}", path.display())))?;
    Ok(json!({
        "ok": true,
        "path": path.display().to_string(),
        "bytes": body.len(),
    }))
}

fn run_fcpxml<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let path = output_path(p, "export.fcpxml")?;
        let timeline = s
            .services()
            .project
            .current_timeline()
            .await
            .ok_or(EngineError::NoProject)?;
        let name = s
            .services()
            .project
            .current_project()
            .await
            .map(|project| project.meta.name)
            .unwrap_or_else(|| "Snapshort".into());
        let rate = rate_of(s, &timeline);
        let xml = snapshort_interchange::export_fcpxml(&name, &timeline, rate, &|path: &str| {
            source_name(path)
        });
        write_out(&path, &xml, "export.fcpxml")
    })
}

fn run_edl<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let path = output_path(p, "export.edl")?;
        let timeline = s
            .services()
            .project
            .current_timeline()
            .await
            .ok_or(EngineError::NoProject)?;
        let name = s
            .services()
            .project
            .current_project()
            .await
            .map(|project| project.meta.name)
            .unwrap_or_else(|| "Snapshort".into());
        let rate = rate_of(s, &timeline);
        let edl = snapshort_interchange::export_edl(&name, &timeline, rate);
        write_out(&path, &edl, "export.edl")
    })
}

fn run_srt<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let path = output_path(p, "export.srt")?;
        let timeline = s
            .services()
            .project
            .current_timeline()
            .await
            .ok_or(EngineError::NoProject)?;
        let srt = snapshort_interchange::export_srt(&timeline);
        write_out(&path, &srt, "export.srt")
    })
}

/// Bring an SRT file in as timed text clips on a text track.
fn run_import_captions<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let path = params::require_file(p, "path", "captions.import")?;
        let body = std::fs::read_to_string(&path).map_err(|e| {
            EngineError::bad(
                "captions.import",
                format!("cannot read {}: {e}", path.display()),
            )
        })?;
        let cues = snapshort_interchange::parse_srt(&body);
        if cues.is_empty() {
            return Err(EngineError::bad(
                "captions.import",
                format!("no cues found in {}", path.display()),
            ));
        }
        let fps = s.fps();
        let start = params::time_at(p, "start", fps).unwrap_or(miniter_domain::Timestamp::ZERO);
        let clips =
            snapshort_interchange::clips_from_cues(&cues, miniter_domain::TextStyle::default());
        let timeline = s
            .services()
            .project
            .current_timeline()
            .await
            .ok_or(EngineError::NoProject)?;
        let track_id = match params::track_p(p, "track") {
            Some(id) => id,
            None => timeline
                .tracks
                .iter()
                .find(|t| t.kind == miniter_domain::TrackKind::Text && !t.locked)
                .map(|t| t.id)
                .ok_or_else(|| EngineError::bad("captions.import", "no unlocked text track"))?,
        };
        let commands: Vec<miniter_usecases::EditCommand> = clips
            .into_iter()
            .map(|mut clip| {
                clip.timeline_start = miniter_domain::Timestamp::from_micros(
                    clip.timeline_start.as_micros() + start.as_micros(),
                );
                miniter_usecases::EditCommand::AddClip { track_id, clip }
            })
            .collect();
        let cues_seen = commands.len();
        crate::commands::apply_batch(s, commands, "Import Captions").await?;
        Ok(json!({
            "ok": true,
            "cues": cues_seen,
            "track": track_id.0.to_string(),
        }))
    })
}

fn run_formats<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let timeline = s.services().project.current_timeline().await;
        let rate = timeline
            .as_ref()
            .map(|timeline| rate_of(s, timeline).as_f64())
            .unwrap_or(s.fps() as f64);
        Ok(json!({
            "export": {
                "fcpxml": {"extension": "fcpxml", "label": "Final Cut Pro XML"},
                "edl": {"extension": "edl", "label": "CMX3600 EDL"},
                "srt": {"extension": "srt", "label": "SubRip captions"},
            },
            "frameRate": rate,
        }))
    })
}

/// Names a source after its stem, for tooltips and exports.
pub fn describe_source(path: &str) -> String {
    require_str(&json!({}), "path", "export.fcpxml")
        .map(|_| source_name(path))
        .unwrap_or_else(|_| source_name(path))
}
