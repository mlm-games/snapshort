//! The UI half of the control channel: what an agent can inspect and drive in
//! the live app.
//!
//! Requests arrive on a queue the view pump drains each frame, so every
//! handler runs on the UI thread and may read and write the app's signals
//! directly. Anything that changes the document goes through the same
//! backend dispatch the views use, which keeps it undoable and keeps previews
//! and exports consistent.

use crate::state::Store;
use miniter_domain::{
    BlendMode, Clip, ClipId, ClipKind, KeyframeCurve, MediaDuration, TextOverlay, TextStyle,
    Timestamp, TrackId, TrackKind,
};
use miniter_usecases::EditCommand;
use repose_core::request_frame;
use serde_json::{Value, json};
use snapshort_usecases::{PlaybackCommand, ProjectCommand, RenderCommand};

use super::pickers;
use super::views::panels;
use super::views::timeline::{self, geometry};

/// The app data dir: OS dirs on desktop, internal storage on Android. Shared
/// so both backends and the control channel agree on where data lives.
pub fn app_data_dir() -> Option<std::path::PathBuf> {
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    {
        directories::ProjectDirs::from("com", "mlm-games", "snapshort")
            .map(|d| d.data_dir().to_path_buf())
    }
    #[cfg(target_os = "android")]
    {
        Some(game_utils::android_data_dir("org.mlm.snapshort"))
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

/// One request the app answers on its UI thread.
pub struct UiRequest {
    pub method: String,
    pub params: Value,
    pub reply: flume::Sender<Result<Value, String>>,
}

/// Both ends of the UI request queue.
#[derive(Clone)]
pub struct UiChannel {
    tx: flume::Sender<UiRequest>,
    rx: flume::Receiver<UiRequest>,
}

impl UiChannel {
    pub fn new() -> Self {
        let (tx, rx) = flume::unbounded();
        Self { tx, rx }
    }

    /// Ask the UI thread and wait for its answer (a failed or late reply is an
    /// error, never a panic).
    pub fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let (reply_tx, reply_rx) = flume::bounded(1);
        if self
            .tx
            .send(UiRequest {
                method: method.to_string(),
                params,
                reply: reply_tx,
            })
            .is_err()
        {
            return Err("the app is shutting down".into());
        }
        request_frame();
        reply_rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|_| "the app did not answer in time".to_string())?
    }

    /// Drain pending requests. Called from the view pump, on the UI thread.
    pub fn drain(&self, store: &Store) {
        while let Ok(request) = self.rx.try_recv() {
            let result = handle(store, &request.method, &request.params);
            let _ = request.reply.send(result);
            request_frame();
        }
    }
}

/// Dispatch one `ui.*` method.
pub fn handle(store: &Store, method: &str, params: &Value) -> Result<Value, String> {
    match method {
        "ui.inspect" => Ok(inspect(store)),
        "ui.elements" => Ok(elements(store, string(params.get("prefix")).as_deref())),
        "ui.click" => click(store, params),
        "ui.drag" => drag(store, params),
        "ui.key" => key(
            store,
            string(params.get("key")).as_deref().unwrap_or_default(),
        ),
        "ui.type" => type_text(
            store,
            string(params.get("text")).as_deref().unwrap_or_default(),
        ),
        "ui.menu" => menu(
            store,
            string(params.get("id")).as_deref().unwrap_or_default(),
        ),
        "ui.scroll" => scroll(store, params),
        "ui.playback" => playback(store, params),
        other => Err(format!("unknown method `{other}`")),
    }
}

fn string(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::to_string)
}

fn clip_uuid(id: &str) -> Option<ClipId> {
    id.strip_prefix("timeline.clip.")
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .map(ClipId)
}

fn track_uuid(id: &str) -> Option<TrackId> {
    id.strip_prefix("timeline.track.")
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .map(TrackId)
}

/// The tabs node that hosts `panel`, so `set_active` can target it.
fn tabs_node_of(node: &repose_docking::DockNode, panel: repose_docking::PanelId) -> Option<u64> {
    match &node.kind {
        repose_docking::DockKind::Tabs { tabs, .. } => tabs.contains(&panel).then_some(node.id),
        repose_docking::DockKind::Split { a, b, .. } => {
            tabs_node_of(a, panel).or_else(|| tabs_node_of(b, panel))
        }
        _ => None,
    }
}

fn activate_panel(store: &Store, name: &str) -> Result<Value, String> {
    let Some(panel) = panel_id(name) else {
        return Err(format!("no panel named `{name}`"));
    };
    let mut dock = store.dock_state.borrow_mut();
    let node = tabs_node_of(&dock.root, panel);
    if let Some(node) = node {
        dock.set_active(node, panel);
    }
    drop(dock);
    request_frame();
    Ok(json!({"ok": true, "activated": name}))
}

fn panel_id(name: &str) -> Option<repose_docking::PanelId> {
    Some(match name {
        "project" | "assets" => panels::PANEL_PROJECT,
        "media" | "browser" => panels::PANEL_MEDIA_BROWSER,
        "effects" => panels::PANEL_EFFECTS,
        "program" | "monitor" => panels::PANEL_PROGRAM_MONITOR,
        "source" => panels::PANEL_SOURCE_MONITOR,
        "timeline" => panels::PANEL_TIMELINE,
        "inspector" => panels::PANEL_INSPECTOR,
        "history" => panels::PANEL_HISTORY,
        "mixer" | "audio" => panels::PANEL_AUDIO_MIXER,
        "export" => panels::PANEL_EXPORT,
        _ => return None,
    })
}

fn inspect(store: &Store) -> Value {
    let state = &store.state;
    let (tracks, clips, duration_us) = state
        .timeline
        .get()
        .map(|t| {
            (
                t.tracks.len(),
                t.tracks.iter().map(|tr| tr.clips.len()).sum::<usize>(),
                t.duration_end().as_micros(),
            )
        })
        .unwrap_or((0, 0, 0));
    let panels_json: Vec<Value> = PANEL_TITLES
        .iter()
        .map(|(id, title)| json!({"id": id, "title": title}))
        .collect();

    json!({
        "app": "snapshort",
        "project": {
            "name": state.project.get().map(|p| p.meta.name),
            "path": state.project_path.get().map(|p| p.display().to_string()),
            "dirty": state.project_dirty.get(),
        },
        "timeline": {"tracks": tracks, "clips": clips, "durationUs": duration_us},
        "playhead": {
            "us": state.playhead.get().as_micros(),
            "seconds": state.playhead.get().as_secs_f64(),
        },
        "playback": state.playback_state.get(),
        "selection": {"clip": state.selected_clip_id.get().map(|c| c.0.to_string())},
        "zoom": state.timeline_zoom.get(),
        "snap": state.timeline_snap.get(),
        "markers": state.timeline_markers.get().iter().map(|m| json!({
            "us": m.timestamp_us,
            "label": m.label,
        })).collect::<Vec<_>>(),
        "export": {
            "progress": state.render_progress.get(),
            "output": state.export_output_path.get().map(|p| p.display().to_string()),
            "quality": format!("{:?}", state.export_quality.get()),
            "lastResult": state.last_render_result.get(),
        },
        "monitor": {"preferProxy": state.prefer_proxy.get()},
        "autoProxy": state.auto_proxy.get(),
        "status": state.status_msg.get(),
        "canUndo": state.can_undo.get(),
        "canRedo": state.can_redo.get(),
        "panels": panels_json,
    })
}

/// Every registered panel as `(id, title)`.
const PANEL_TITLES: &[(repose_docking::PanelId, &str)] = &[
    (panels::PANEL_PROJECT, "Project"),
    (panels::PANEL_MEDIA_BROWSER, "Media Browser"),
    (panels::PANEL_EFFECTS, "Effects"),
    (panels::PANEL_PROGRAM_MONITOR, "Program"),
    (panels::PANEL_SOURCE_MONITOR, "Source"),
    (panels::PANEL_TIMELINE, "Timeline"),
    (panels::PANEL_INSPECTOR, "Inspector"),
    (panels::PANEL_HISTORY, "History"),
    (panels::PANEL_AUDIO_MIXER, "Audio Mixer"),
    (panels::PANEL_EXPORT, "Export"),
];

fn elements(store: &Store, prefix: Option<&str>) -> Value {
    let prefix = prefix.unwrap_or_default();
    let mut items: Vec<Value> = Vec::new();

    for name in [
        "project",
        "media",
        "effects",
        "program",
        "source",
        "timeline",
        "inspector",
        "history",
        "mixer",
        "export",
    ] {
        let id = format!("panel.{name}");
        if id.starts_with(prefix) {
            items.push(json!({"id": id, "label": name, "rect": Value::Null}));
        }
    }
    for action in [
        "play",
        "pause",
        "stop",
        "start",
        "end",
        "stepBack",
        "stepForward",
    ] {
        let id = format!("transport.{action}");
        if id.starts_with(prefix) {
            items.push(json!({"id": id, "label": action, "rect": Value::Null}));
        }
    }

    if let Some(timeline) = store.state.timeline.get() {
        let ruler = geometry::RULER_HEIGHT;
        let track_h = geometry::TRACK_HEIGHT;
        if "timeline.ruler".starts_with(prefix) {
            items.push(json!({
                "id": "timeline.ruler",
                "label": "Ruler",
                "rect": {"x": 0.0, "y": 0.0, "w": 1_000_000.0, "h": ruler},
            }));
        }
        for (index, track) in timeline.tracks.iter().enumerate() {
            let id = format!("timeline.track.{}", track.id.0);
            if id.starts_with(prefix) {
                items.push(json!({
                    "id": id,
                    "label": track.name,
                    "kind": format!("{:?}", track.kind),
                    "rect": {"x": 0.0, "y": ruler + index as f32 * track_h, "w": 0.0, "h": track_h},
                }));
            }
            for clip in &track.clips {
                let id = format!("timeline.clip.{}", clip.id.0);
                if id.starts_with(prefix) {
                    items.push(json!({
                        "id": id,
                        "label": geometry::clip_label(clip),
                        "track": track.id.0.to_string(),
                        "rect": {
                            "x": clip.timeline_start.as_micros() as f64,
                            "y": ruler + index as f32 * track_h,
                            "w": clip.timeline_duration.as_micros() as f64,
                            "h": track_h,
                            "timeUnit": "us",
                        },
                    }));
                }
            }
        }
    }

    json!({"elements": items, "count": items.len()})
}

/// Where a pointer at window coordinates lands in the timeline, if anywhere.
fn timeline_hit(store: &Store, x: f64, y: f64) -> Option<(usize, Timestamp)> {
    let scale = geometry::TimelineScale::new(store.state.timeline_zoom.get());
    let header = geometry::TRACK_HEADER_WIDTH as f64;
    if x <= header {
        return None;
    }
    let us = ((x - header).max(0.0) / scale.px_per_us as f64) as i64;
    let ruler = geometry::RULER_HEIGHT as f64;
    let index = ((y - ruler).max(0.0) / geometry::TRACK_HEIGHT as f64).floor() as usize;
    Some((index, Timestamp::from_micros(us)))
}

fn resolve_target(store: &Store, value: Option<&Value>) -> Option<Value> {
    let value = value?;
    if let Some(id) = string(value.get("id")) {
        if let Some(clip) = clip_uuid(&id) {
            let (track, _) = find_clip(store, clip)?;
            return Some(
                json!({"kind": "clip", "id": id, "clip": clip.0.to_string(), "track": track.0.to_string()}),
            );
        }
        if let Some(track) = track_uuid(&id) {
            return Some(json!({"kind": "track", "id": id, "track": track.0.to_string()}));
        }
        if id == "timeline.ruler" || id.starts_with("timeline.") {
            return Some(json!({"kind": "timeline", "id": id}));
        }
        if id.starts_with("panel.") {
            return Some(json!({"kind": "panel", "id": id}));
        }
        if id.starts_with("transport.") {
            return Some(json!({"kind": "transport", "id": id}));
        }
        return Some(json!({"kind": "command", "id": id}));
    }
    let x = value.get("x").and_then(Value::as_f64)?;
    let y = value.get("y").and_then(Value::as_f64)?;
    let (index, at) = timeline_hit(store, x, y)?;
    let timeline = store.state.timeline.get()?;
    let track = timeline.tracks.get(index)?;
    let clip = track.clip_at(at).map(|c| c.id);
    Some(json!({
        "kind": "point",
        "x": x,
        "y": y,
        "track": track.id.0.to_string(),
        "us": at.as_micros(),
        "clip": clip.map(|c| c.0.to_string()),
    }))
}

fn find_clip(store: &Store, clip_id: ClipId) -> Option<(TrackId, Clip)> {
    let timeline = store.state.timeline.get()?;
    timeline
        .tracks
        .iter()
        .find_map(|t| t.clip_by_id(clip_id).map(|c| (t.id, c.clone())))
}

fn click(store: &Store, params: &Value) -> Result<Value, String> {
    let Some(target) = resolve_target(store, Some(params)) else {
        return Err("give `id`, or `x` and `y` in window points".into());
    };
    let count = params.get("count").and_then(Value::as_u64).unwrap_or(1);
    match target["kind"].as_str().unwrap_or_default() {
        "panel" => {
            let name = target["id"]
                .as_str()
                .unwrap_or_default()
                .trim_start_matches("panel.");
            activate_panel(store, name)
        }
        "transport" => {
            let action = target["id"]
                .as_str()
                .unwrap_or_default()
                .trim_start_matches("transport.");
            playback_action(store, action)
        }
        "clip" => {
            let clip = clip_uuid(target["id"].as_str().unwrap_or_default()).ok_or("bad clip id")?;
            store.state.selected_clip_id.set(Some(clip));
            store.state.selected_asset_id.set(None);
            Ok(json!({"ok": true, "selected": target["id"]}))
        }
        "track" => {
            let track =
                track_uuid(target["id"].as_str().unwrap_or_default()).ok_or("bad track id")?;
            store.dispatch_selection(Vec::new(), vec![track]);
            Ok(json!({"ok": true, "selected": target["id"]}))
        }
        "timeline" | "point" => {
            let us = target["us"].as_i64().unwrap_or(0);
            if let Some(clip) = target.get("clip").and_then(Value::as_str) {
                let clip = clip_uuid(&format!("timeline.clip.{clip}")).ok_or("bad clip id")?;
                store.state.selected_clip_id.set(Some(clip));
                if count >= 2 {
                    store.state.inspector_flags.borrow_mut().clear();
                }
                return Ok(json!({"ok": true, "selected": format!("timeline.clip.{}", clip.0)}));
            }
            store.state.selected_clip_id.set(None);
            store.dispatch_playback(PlaybackCommand::Seek {
                timestamp: Timestamp::from_micros(us),
            });
            Ok(json!({"ok": true, "seekUs": us}))
        }
        "command" => menu(store, target["id"].as_str().unwrap_or_default()),
        _ => Err("could not resolve that target".into()),
    }
}

fn drag(store: &Store, params: &Value) -> Result<Value, String> {
    let from = resolve_target(store, params.get("from")).ok_or("`from` needs `id` or `x`,`y`")?;
    let to = resolve_target(store, params.get("to")).ok_or("`to` needs `id` or `x`,`y`")?;

    if from["kind"].as_str() == Some("clip") {
        let clip = clip_uuid(from["id"].as_str().unwrap_or_default()).ok_or("bad clip id")?;
        let (track_id, current) = find_clip(store, clip).ok_or("clip not found")?;
        let start = current.timeline_start.as_micros();
        let end = current.timeline_end().as_micros();
        let us = to.get("us").and_then(Value::as_i64).unwrap_or(start);
        let scale = geometry::TimelineScale::new(store.state.timeline_zoom.get());
        let handle_us = (geometry::TRIM_HANDLE_WIDTH as f64 / scale.px_per_us as f64) as i64;

        let command = if to["kind"].as_str() == Some("track") {
            let target_track =
                track_uuid(to["id"].as_str().unwrap_or_default()).ok_or("bad track id")?;
            return move_clip(store, clip, target_track, us);
        } else if (us - start).abs() <= handle_us {
            EditCommand::TrimClipStart {
                clip_id: clip,
                new_start: Timestamp::from_micros(us.clamp(0, end - 1_000)),
                new_source_start: MediaDuration::from_micros(
                    (current.source_start.as_micros()
                        + ((us - start) as f64 / current.speed) as i64)
                        .max(0),
                ),
            }
        } else if (us - end).abs() <= handle_us {
            EditCommand::TrimClipEnd {
                clip_id: clip,
                new_duration: MediaDuration::from_micros((us - start).max(1_000)),
            }
        } else {
            return move_clip(store, clip, track_id, us);
        };
        store.dispatch_edit(command);
        let _ = track_id;
        return Ok(json!({"ok": true, "trimmed": clip.0.to_string()}));
    }

    let us = to.get("us").and_then(Value::as_i64).unwrap_or(0);
    store.dispatch_playback(PlaybackCommand::Seek {
        timestamp: Timestamp::from_micros(us),
    });
    Ok(json!({"ok": true, "seekUs": us}))
}

fn move_clip(store: &Store, clip: ClipId, track: TrackId, us: i64) -> Result<Value, String> {
    store.dispatch_edit(EditCommand::MoveClip {
        clip_id: clip,
        new_track_id: track,
        new_start: Timestamp::from_micros(us.max(0)),
    });
    Ok(json!({"ok": true, "moved": clip.0.to_string(), "track": track.0.to_string()}))
}

fn key(store: &Store, key: &str) -> Result<Value, String> {
    let key = normalize_key(key);
    match key.as_str() {
        "space" => playback_action(store, "play"),
        "left" => step(store, -1).map(|_| json!({"ok": true})),
        "right" => step(store, 1).map(|_| json!({"ok": true})),
        "home" => seek_seconds(store, 0.0).map(|_| json!({"ok": true})),
        "end" => seek_end(store).map(|_| json!({"ok": true})),
        "delete" | "backspace" => delete_selected(store).map(|_| json!({"ok": true})),
        "s" => split_at_playhead(store).map(|_| json!({"ok": true})),
        "cmd+z" => undo(store).map(|_| json!({"ok": true})),
        "cmd+shift+z" | "ctrl+y" => redo(store).map(|_| json!({"ok": true})),
        "cmd+s" => save(store).map(|_| json!({"ok": true})),
        "cmd+d" => duplicate_selected(store).map(|_| json!({"ok": true})),
        "cmd+e" => menu(store, "file.export"),
        "cmd+i" => menu(store, "file.import"),
        "+" | "=" => zoom(store, 1.25).map(|_| json!({"ok": true})),
        "-" => zoom(store, 0.8).map(|_| json!({"ok": true})),
        other => return Err(format!("no action for `{other}`")),
    }
    .map(|_| json!({"ok": true, "key": key}))
}

fn normalize_key(key: &str) -> String {
    let mut parts: Vec<String> = key
        .split('+')
        .map(|p| p.trim().to_ascii_lowercase())
        .collect();
    let last = parts.pop().unwrap_or_default();
    let last = match last.as_str() {
        " " => "space".to_string(),
        "esc" => "escape".to_string(),
        "return" | "enter" => "enter".to_string(),
        other => other.to_string(),
    };
    let mut out = String::new();
    if parts
        .iter()
        .any(|p| p == "cmd" || p == "ctrl" || p == "meta")
    {
        out.push_str("cmd+");
    }
    if parts.iter().any(|p| p == "shift") {
        out.push_str("shift+");
    }
    out.push_str(&last);
    out
}

fn step(store: &Store, frames: i64) -> Result<(), String> {
    let current = store.state.playhead.get().as_micros();
    let next = (current + 41_666 * frames).max(0);
    store.dispatch_playback(PlaybackCommand::Seek {
        timestamp: Timestamp::from_micros(next),
    });
    Ok(())
}

fn seek_seconds(store: &Store, seconds: f64) -> Result<(), String> {
    store.dispatch_playback(PlaybackCommand::Seek {
        timestamp: Timestamp::from_secs(seconds),
    });
    Ok(())
}

fn seek_end(store: &Store) -> Result<(), String> {
    let end = store
        .state
        .timeline
        .get()
        .map(|t| t.duration_end().as_secs_f64())
        .unwrap_or(0.0);
    seek_seconds(store, end)
}

fn delete_selected(store: &Store) -> Result<(), String> {
    let Some(clip_id) = store.state.selected_clip_id.get() else {
        return Err("no clip is selected".into());
    };
    store.dispatch_edit(EditCommand::RemoveClip { clip_id });
    store.state.selected_clip_id.set(None);
    Ok(())
}

fn split_at_playhead(store: &Store) -> Result<(), String> {
    let Some(clip_id) = store.state.selected_clip_id.get() else {
        return Err("no clip is selected".into());
    };
    let Some(timeline) = store.state.timeline.get() else {
        return Err("no timeline is open".into());
    };
    let at = store.state.playhead.get();
    if !timeline::can_split_clip(&timeline, clip_id, at) {
        return Err("the playhead is not inside the selected clip".into());
    }
    store.dispatch_edit(EditCommand::SplitClip {
        clip_id,
        at,
        new_clip_id: ClipId::new(),
    });
    Ok(())
}

fn undo(store: &Store) -> Result<(), String> {
    store.dispatch_undo();
    Ok(())
}

fn redo(store: &Store) -> Result<(), String> {
    store.dispatch_redo();
    Ok(())
}

fn save(store: &Store) -> Result<(), String> {
    store.dispatch_project(ProjectCommand::Save {
        markers: Vec::new(),
    });
    Ok(())
}

fn duplicate_selected(store: &Store) -> Result<(), String> {
    let Some(clip_id) = store.state.selected_clip_id.get() else {
        return Err("no clip is selected".into());
    };
    let Some((track_id, clip)) = find_clip(store, clip_id) else {
        return Err("clip not found".into());
    };
    store.dispatch_edit(EditCommand::DuplicateClip {
        source_clip_id: clip_id,
        new_clip_id: ClipId::new(),
        target_track_id: track_id,
        target_start: clip.timeline_end(),
    });
    Ok(())
}

fn zoom(store: &Store, factor: f32) -> Result<(), String> {
    let zoom = (store.state.timeline_zoom.get() * factor).clamp(0.1, 40.0);
    store.state.timeline_zoom.set(zoom);
    request_frame();
    Ok(())
}

fn type_text(store: &Store, text: &str) -> Result<Value, String> {
    store.state.asset_search_query.set(text.to_string());
    Ok(json!({"ok": true, "assetSearchQuery": text}))
}

fn scroll(store: &Store, params: &Value) -> Result<Value, String> {
    let dy = params.get("dy").and_then(Value::as_f64).unwrap_or(0.0);
    if dy.abs() > f64::EPSILON {
        let factor = (1.0 + dy as f32 * 0.001).clamp(0.5, 2.0);
        let zoom = (store.state.timeline_zoom.get() * factor).clamp(0.1, 40.0);
        store.state.timeline_zoom.set(zoom);
        request_frame();
    }
    Ok(json!({"ok": true, "zoom": store.state.timeline_zoom.get()}))
}

fn playback(store: &Store, params: &Value) -> Result<Value, String> {
    let action = string(params.get("action")).unwrap_or_else(|| "inspect".into());
    playback_action(store, &action)
}

fn playback_action(store: &Store, action: &str) -> Result<Value, String> {
    match action {
        "play" => store.dispatch_playback(PlaybackCommand::Play),
        "pause" => store.dispatch_playback(PlaybackCommand::Pause),
        "stop" => store.dispatch_playback(PlaybackCommand::Stop),
        "start" => seek_seconds(store, 0.0)?,
        "end" => seek_end(store)?,
        "stepBack" => step(store, -1)?,
        "stepForward" => step(store, 1)?,
        other => return Err(format!("unknown playback action `{other}`")),
    }
    Ok(json!({"ok": true, "action": action}))
}

fn menu(store: &Store, id: &str) -> Result<Value, String> {
    match id {
        "file.import" => {
            pickers::start_picker(
                store,
                pickers::ActivePicker::ImportMedia(pickers::pick_media_files()),
            );
            Ok(json!({"ok": true, "opened": "import picker"}))
        }
        "file.save" => save(store).map(|_| json!({"ok": true})),
        "file.saveAs" => {
            let name = store
                .state
                .project
                .get()
                .map(|p| p.meta.name)
                .unwrap_or_else(|| "Untitled".into());
            pickers::start_picker(
                store,
                pickers::ActivePicker::SaveProject {
                    picker: pickers::pick_save_project(&name),
                    markers: Vec::new(),
                },
            );
            Ok(json!({"ok": true, "opened": "save picker"}))
        }
        "file.export" => {
            pickers::start_picker(
                store,
                pickers::ActivePicker::ExportPath(pickers::pick_export_path()),
            );
            Ok(json!({"ok": true, "opened": "export picker"}))
        }
        "file.open" => {
            pickers::start_picker(
                store,
                pickers::ActivePicker::OpenProject(pickers::pick_open_project()),
            );
            Ok(json!({"ok": true, "opened": "open picker"}))
        }
        "project.new" => {
            store.dispatch_project(ProjectCommand::Create {
                name: "Untitled".into(),
            });
            Ok(json!({"ok": true}))
        }
        "timeline.split" => split_at_playhead(store).map(|_| json!({"ok": true})),
        "timeline.delete" => delete_selected(store).map(|_| json!({"ok": true})),
        "timeline.addTrack" => add_track(store, TrackKind::Video),
        "timeline.addAudioTrack" => add_track(store, TrackKind::Audio),
        "timeline.addText" => add_text(store),
        "timeline.addSubtitles" | "captions.import" => {
            pickers::start_picker(
                store,
                pickers::ActivePicker::ImportSubtitleFile(pickers::pick_subtitle_file()),
            );
            Ok(json!({"ok": true, "opened": "caption file picker"}))
        }
        "preset.list" => Ok(json!({
            "ok": true,
            "presets": snapshort_engine::preset_names(&app_data_dir().unwrap_or_default()),
        })),
        "app.undo" => undo(store).map(|_| json!({"ok": true})),
        "app.redo" => redo(store).map(|_| json!({"ok": true})),
        "render.export" => {
            store.dispatch_render(RenderCommand::PreparePlan);
            Ok(json!({"ok": true, "opened": "export plan"}))
        }
        "render.cancel" => {
            store.dispatch_render(RenderCommand::CancelExport);
            Ok(json!({"ok": true}))
        }
        "preview.toggleProxy" => {
            let prefer = !store.state.prefer_proxy.get();
            store.dispatch_preview(snapshort_usecases::PreviewCommand::SetPreferProxy { prefer });
            Ok(json!({"ok": true, "preferProxy": prefer}))
        }
        other => Err(format!("no UI action for `{other}`")),
    }
}

fn add_track(store: &Store, kind: TrackKind) -> Result<Value, String> {
    let name = store
        .state
        .timeline
        .get()
        .map(|t| timeline::next_track_name(&t, kind))
        .unwrap_or_else(|| format!("{kind:?} 1"));
    store.dispatch_edit(EditCommand::AddTrack { kind, name });
    Ok(json!({"ok": true}))
}

fn add_text(store: &Store) -> Result<Value, String> {
    let timeline = store.state.timeline.get().ok_or("no timeline is open")?;
    let track_id = timeline
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Text && !t.locked)
        .map(|t| t.id)
        .ok_or("no unlocked text track")?;
    let duration = MediaDuration::from_secs(5.0);
    let clip = Clip {
        id: ClipId::new(),
        timeline_start: store.state.playhead.get(),
        timeline_duration: duration,
        source_start: MediaDuration::ZERO,
        source_end: duration,
        source_total_duration: duration,
        speed: 1.0,
        volume: 1.0,
        opacity: 1.0,
        muted: false,
        transition_in: None,
        transition_out: None,
        markers: Vec::new(),
        kind: ClipKind::Text(TextOverlay {
            text: "Text".into(),
            style: TextStyle::default(),
        }),
        keyframes: KeyframeCurve::default(),
        blend_mode: BlendMode::Normal,
    };
    store.dispatch_edit(EditCommand::AddClip { track_id, clip });
    Ok(json!({"ok": true}))
}
