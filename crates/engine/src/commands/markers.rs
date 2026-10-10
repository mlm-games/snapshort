//! Timeline markers: named, coloured, and addressable by agents.
//!
//! The session owns the list (it is part of a saved project); the UI mirrors
//! it into its own signal and draws it on the ruler.

use serde_json::{Value, json};
use snapshort_usecases::TimelineMarkerData;

use crate::commands::cmd;
use crate::params;
use crate::{EngineError, Result, Session};

use super::{RunFuture, has_project, query};

pub fn specs() -> Vec<crate::commands::CommandSpec> {
    vec![
        cmd!(
            "marker.add",
            "Add Marker",
            ["Timeline", "Add Marker"],
            Some("M"),
            "{\"seconds\"?: number, \"us\"?: int, \"ms\"?: number, \"frame\"?: int, \"label\"?: string, \"color\"?: string, \"duration\"?: time}",
            has_project,
            run_add
        ),
        cmd!(
            "marker.remove",
            "Remove Marker",
            [],
            None::<&'static str>,
            "{\"index\"?: int, \"at\"?: time}",
            has_project,
            run_remove
        ),
        cmd!(
            "marker.rename",
            "Rename Marker",
            [],
            None::<&'static str>,
            "{\"index\"?: int, \"at\"?: time, \"label\": string}",
            has_project,
            run_rename
        ),
        query!("marker.list", "Markers", "{}", run_list),
        cmd!(
            "marker.clear",
            "Clear Markers",
            [],
            None::<&'static str>,
            "{}",
            has_project,
            run_clear
        ),
    ]
}

fn marker_json(marker: &TimelineMarkerData) -> Value {
    json!({
        "index": 0,
        "us": marker.timestamp_us,
        "seconds": marker.timestamp_us as f64 / 1_000_000.0,
        "label": marker.label,
        "color": marker.color,
        "durationUs": marker.duration_us,
    })
}

fn markers_json(s: &Session) -> Value {
    let markers = s.markers();
    let items: Vec<Value> = markers
        .iter()
        .enumerate()
        .map(|(index, marker)| {
            let mut value = marker_json(marker);
            value["index"] = json!(index);
            value
        })
        .collect();
    json!({"markers": items, "count": items.len()})
}

/// Which marker a command means: by index, or the one at/covering a time.
fn marker_at(s: &Session, p: &Value, cmd: &str) -> Result<usize> {
    if let Some(index) = params::usize_p(p, "index") {
        return Ok(index);
    }
    let fps = s.fps();
    let at = params::time_at(p, "at", fps)
        .or_else(|| params::time_p(p, fps))
        .ok_or_else(|| EngineError::bad(cmd, "give `index` or `at`"))?
        .as_micros();
    let markers = s.markers();
    markers
        .iter()
        .position(|m| m.timestamp_us <= at && at < m.timestamp_us + m.duration_us.max(1))
        .or_else(|| {
            markers
                .iter()
                .enumerate()
                .min_by_key(|(_, m)| (m.timestamp_us - at).abs())
                .map(|(index, _)| index)
        })
        .ok_or_else(|| EngineError::Other("there are no markers".into()))
}

fn run_add<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let fps = s.fps();
        let playhead = s.services().project.current_playhead().await;
        let at = params::time_at(p, "at", fps)
            .or_else(|| params::time_p(p, fps))
            .unwrap_or(playhead);
        let label = params::str_p(p, "label").unwrap_or("Marker").to_string();
        let color = params::str_p(p, "color").unwrap_or_default().to_string();
        let duration = params::duration_at(p, "duration", fps).unwrap_or_default();

        let marker = TimelineMarkerData {
            timestamp_us: at.as_micros(),
            label,
            color,
            duration_us: duration.as_micros(),
        };
        {
            let mut markers = s.markers();
            markers.push(marker.clone());
            markers.sort_by_key(|m| m.timestamp_us);
        }
        s.note_markers_changed().await;
        Ok(marker_json(&marker))
    })
}

fn run_remove<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let index = marker_at(s, p, "marker.remove")?;
        let removed = {
            let mut markers = s.markers();
            if index >= markers.len() {
                return Err(EngineError::bad("marker.remove", "no such marker"));
            }
            markers.remove(index)
        };
        s.note_markers_changed().await;
        Ok(marker_json(&removed))
    })
}

fn run_rename<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let index = marker_at(s, p, "marker.rename")?;
        let label = params::require_str(p, "label", "marker.rename")?.to_string();
        let updated = {
            let mut markers = s.markers();
            let Some(marker) = markers.get_mut(index) else {
                return Err(EngineError::bad("marker.rename", "no such marker"));
            };
            marker.label = label;
            marker.clone()
        };
        s.note_markers_changed().await;
        Ok(marker_json(&updated))
    })
}

fn run_list<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move { Ok(markers_json(s)) })
}

fn run_clear<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        s.markers().clear();
        s.note_markers_changed().await;
        Ok(json!({"ok": true}))
    })
}
