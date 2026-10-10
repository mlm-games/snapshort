//! Long exports over MCP: `render.export` with `wait: true` runs as a
//! background job; the server reports progress as MCP
//! `notifications/progress` and stops it on `notifications/cancelled`, deleting
//! the partial output.

use std::path::Path;
use std::time::{Duration, SystemTime};

use rmcp::RoleServer;
use rmcp::model::{
    CallToolResult, ContentBlock as Content, ProgressNotificationParam, ProgressToken,
};
use rmcp::service::{Peer, RequestContext};
use serde_json::{Value, json};

use crate::server::SnapshortMcp;

/// Commands that block until a render is written when called with `wait: true`.
pub const LONG_COMMANDS: &[&str] = &["render.export"];

const POLL: Duration = Duration::from_millis(100);

/// Whether `command_run {id, params}` is a long call this module runs.
pub fn is_long(id: &str, params: &Value) -> bool {
    LONG_COMMANDS.contains(&id) && params.get("wait").and_then(Value::as_bool) == Some(true)
}

struct Reporter {
    peer: Peer<RoleServer>,
    token: Option<ProgressToken>,
    last: f64,
}

impl Reporter {
    async fn report(&mut self, done: f64, total: f64, message: &str) {
        let Some(token) = self.token.clone() else {
            return;
        };
        if total <= 0.0 || done <= self.last {
            return;
        }
        self.last = done;
        let mut p = ProgressNotificationParam::new(token, done).with_total(total);
        if !message.is_empty() {
            p = p.with_message(message);
        }
        let _ = self.peer.notify_progress(p).await;
    }
}

impl SnapshortMcp {
    pub(crate) async fn run_long(
        &self,
        id: &str,
        mut params: Value,
        context: &RequestContext<RoleServer>,
    ) -> CallToolResult {
        let started = SystemTime::now();
        if let Some(p) = params.as_object_mut() {
            p.insert("wait".into(), json!(false));
        }
        let start = match self.run(id, params).await {
            Ok(v) => v,
            Err(e) => return CallToolResult::error(vec![Content::text(e.to_string())]),
        };
        let Some(job) = start.get("job").and_then(Value::as_str) else {
            return CallToolResult::error(vec![Content::text(format!(
                "{id} started no job: {start}"
            ))]);
        };
        let output = start
            .get("output")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut rep = Reporter {
            peer: context.peer.clone(),
            token: context.meta.get_progress_token(),
            last: 0.0,
        };
        loop {
            let state = self.job_state(job).await;
            let finished = state
                .as_ref()
                .is_none_or(|j| !matches!(j["status"].as_str(), Some("queued") | Some("running")));
            if let Some(j) = &state {
                let done = j["progress"].as_f64().unwrap_or(0.0);
                rep.report(done, 100.0, j["status"].as_str().unwrap_or_default())
                    .await;
            }
            if finished {
                let result = state.map(|j| j["result"].clone()).unwrap_or(Value::Null);
                if let Some(e) = result.get("error").and_then(Value::as_str) {
                    return CallToolResult::error(vec![Content::text(format!(
                        "export failed: {e}"
                    ))]);
                }
                let mut out = start;
                if let Some(o) = out.as_object_mut() {
                    o.insert("result".into(), result);
                }
                return CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&out).unwrap_or_default(),
                )]);
            }
            tokio::select! {
                _ = context.ct.cancelled() => break,
                _ = tokio::time::sleep(POLL) => {}
            }
        }
        let _ = self.run("jobs.cancel", json!({"job": job})).await;
        for _ in 0..600 {
            if self
                .job_state(job)
                .await
                .is_none_or(|j| !matches!(j["status"].as_str(), Some("queued") | Some("running")))
            {
                break;
            }
            tokio::time::sleep(POLL).await;
        }
        remove_partial(&output, started);
        CallToolResult::error(vec![Content::text("cancelled")])
    }

    async fn job_state(&self, job: &str) -> Option<Value> {
        let jobs = self.run("jobs.list", json!({})).await.ok()?;
        jobs.as_array()?
            .iter()
            .find(|j| j["id"].as_str() == Some(job))
            .cloned()
    }
}

/// Delete what an interrupted export wrote, and nothing else: the output
/// file, the numbered stills of an image sequence (only when the output is
/// one) and the caption sidecar — each only if it was written since `since`.
pub fn remove_partial(path: &str, since: SystemTime) {
    let p = Path::new(path);
    let (Some(dir), Some(stem), Some(file)) = (
        p.parent(),
        p.file_stem().and_then(|s| s.to_str()),
        p.file_name().and_then(|s| s.to_str()),
    ) else {
        return;
    };
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or_default();
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    // File times are coarser than the clock.
    let since = since.checked_sub(Duration::from_secs(1)).unwrap_or(since);
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        if is_export_output(name, file, stem, ext)
            && e.metadata()
                .is_ok_and(|m| m.is_file() && m.modified().is_ok_and(|t| t >= since))
        {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Whether `name` is a file an export to `file` (`stem` + `.ext`) writes.
fn is_export_output(name: &str, file: &str, stem: &str, ext: &str) -> bool {
    if name == file {
        return true;
    }
    let Some(rest) = name.strip_prefix(stem) else {
        return false;
    };
    if let Some(sidecar) = rest.strip_prefix('.') {
        return sidecar.eq_ignore_ascii_case("srt") || sidecar.eq_ignore_ascii_case("vtt");
    }
    if !is_sequence_extension(ext) {
        return false;
    }
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    digits >= 3
        && rest
            .get(digits..)
            .is_some_and(|r| r.strip_prefix('.') == Some(ext))
}

/// Whether an export to a file with extension `ext` writes an image sequence
/// (one still per frame); a movie is a single file.
fn is_sequence_extension(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("png")
        || ext.eq_ignore_ascii_case("jpg")
        || ext.eq_ignore_ascii_case("jpeg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_exports_own_files_are_removed() {
        let dir = std::env::temp_dir().join(format!("snapshort-partial-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let started = SystemTime::now();
        let ours = ["clip.mp4", "clip.srt", "clip.vtt"];
        let theirs = ["clip.aep", "clip2.mp4", "clip000.mp4", "other.mp4"];
        for f in ours.iter().chain(theirs.iter()) {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        remove_partial(&dir.join("clip.mp4").to_string_lossy(), started);
        for f in ours {
            assert!(!dir.join(f).exists(), "{f} should be removed");
        }
        for f in theirs {
            assert!(dir.join(f).exists(), "{f} must not be removed");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn image_sequence_frames_are_removed() {
        let dir = std::env::temp_dir().join(format!("snapshort-seq-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let started = SystemTime::now();
        let ours = ["shot.png", "shot000.png", "shot1234.png"];
        let theirs = ["shot2.png", "shot12.png", "shot.mp4"];
        for f in ours.iter().chain(theirs.iter()) {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        remove_partial(&dir.join("shot.png").to_string_lossy(), started);
        for f in ours {
            assert!(!dir.join(f).exists(), "{f} should be removed");
        }
        for f in theirs {
            assert!(dir.join(f).exists(), "{f} must not be removed");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_older_than_the_export_are_kept() {
        let dir = std::env::temp_dir().join(format!("snapshort-old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("clip.mp4"), b"x").unwrap();
        remove_partial(
            &dir.join("clip.mp4").to_string_lossy(),
            SystemTime::now() + Duration::from_secs(60),
        );
        assert!(dir.join("clip.mp4").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
