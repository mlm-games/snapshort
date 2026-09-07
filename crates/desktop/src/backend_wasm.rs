#![cfg(target_arch = "wasm32")]
//! Web backend: real in-memory editing without native services.
//!
//! Stable wasm has no threads, no sqlite and no media decoders, but everything
//! else is pure Rust: timeline edits apply the miniter reducer directly,
//! projects persist as JSON downloads/uploads, and playback advances by wall
//! clock in the UI pump. Semantics mirror `project_service`, reusing the same
//! `AppEvent`s so the UI layer can't tell the difference.

use crate::state::{BackendCommand, Store};
use miniter_domain::{Project, Timestamp};
use miniter_usecases::reducer::{dispatch, redo, undo, EditorState};
use snapshort_usecases::{
    AppEvent, Asset, AssetId, AssetType, ProjectSnapshot, TimelineMarkerData,
};
use std::collections::HashMap;

const PLAYBACK_FPS: f64 = 24.0;
const STEP_US: i64 = (1_000_000.0 / PLAYBACK_FPS) as i64;

fn video_ext(name: &str) -> bool {
    ["mp4", "mkv", "webm", "mov", "avi"].iter().any(|e| name.ends_with(e))
}

fn audio_ext(name: &str) -> bool {
    ["mp3", "flac", "wav", "ogg", "m4a", "aac"].iter().any(|e| name.ends_with(e))
}

fn image_ext(name: &str) -> bool {
    ["png", "jpg", "jpeg", "webp", "bmp"].iter().any(|e| name.ends_with(e))
}

pub struct WasmBackend {
    editor: Option<EditorState>,
    assets: HashMap<AssetId, Asset>,
    playing: bool,
    last_tick: Option<web_time::Instant>,
    tick_acc_us: i64,
}

impl WasmBackend {
    pub fn new() -> Self {
        Self {
            editor: None,
            assets: HashMap::new(),
            playing: false,
            last_tick: None,
            tick_acc_us: 0,
        }
    }

    /// Apply all queued commands, then advance playback. Runs on the UI thread
    /// from the pump, so signals can be set directly via [`Store::handle_event`].
    pub fn drain(&mut self, store: &Store, cmd_rx: &flume::Receiver<BackendCommand>) {
        while let Ok(cmd) = cmd_rx.try_recv() {
            self.apply(store, cmd);
        }
        self.advance_playback(store);
    }

    fn emit_timeline(&self, store: &Store) {
        if let Some(editor) = self.editor.as_ref() {
            store.handle_event(AppEvent::TimelineUpdated {
                timeline: editor.project.timeline.clone(),
            });
            store.handle_event(AppEvent::UndoStackChanged {
                can_undo: editor.history.can_undo(),
                can_redo: editor.history.can_redo(),
            });
            store.state.project_dirty.set(true);
        }
    }

    fn apply(&mut self, store: &Store, cmd: BackendCommand) {
        match cmd {
            BackendCommand::Project(c) => self.apply_project(store, c),
            BackendCommand::Edit(c) => {
                if let Some(editor) = self.editor.as_mut() {
                    match dispatch(editor, c) {
                        Ok(()) => self.emit_timeline(store),
                        Err(e) => store.state.status_msg.set(format!("Edit failed: {e}")),
                    }
                } else {
                    store.state.status_msg.set("No project open".into());
                }
            }
            BackendCommand::Undo => {
                if let Some(editor) = self.editor.as_mut() {
                    match undo(editor) {
                        Ok(()) => self.emit_timeline(store),
                        Err(e) => store.state.status_msg.set(format!("Undo failed: {e}")),
                    }
                }
            }
            BackendCommand::Redo => {
                if let Some(editor) = self.editor.as_mut() {
                    match redo(editor) {
                        Ok(()) => self.emit_timeline(store),
                        Err(e) => store.state.status_msg.set(format!("Redo failed: {e}")),
                    }
                }
            }
            BackendCommand::Asset(c) => self.apply_asset(store, c),
            BackendCommand::Playback(c) => self.apply_playback(store, c),
            BackendCommand::Preview(_) => {
                // No decoders on web; the monitor keeps its last frame.
            }
            BackendCommand::Render(c) => {
                use snapshort_usecases::RenderCommand;
                match c {
                    RenderCommand::PreparePlan => store
                        .state
                        .status_msg
                        .set("Render plan needs the desktop backend.".into()),
                    RenderCommand::Export { .. } => store
                        .state
                        .status_msg
                        .set("Video export is not available on web yet.".into()),
                }
            }
        }
    }

    fn apply_project(&mut self, store: &Store, cmd: snapshort_usecases::ProjectCommand) {
        use snapshort_usecases::ProjectCommand;
        match cmd {
            ProjectCommand::Create { name } => {
                let project = Project::new(&name);
                let timeline = project.timeline.clone();
                self.editor = Some(EditorState::new(project.clone()));
                self.assets.clear();
                self.stop_playback(store);
                store.handle_event(AppEvent::ProjectCreated { project });
                store.handle_event(AppEvent::TimelineUpdated { timeline });
                store.handle_event(AppEvent::AssetsLoaded { assets: vec![] });
            }
            ProjectCommand::Open { .. } => {
                // Web has no filesystem paths; opens arrive as JSON uploads.
                store.state.status_msg.set(
                    "Use Open to pick a .snap file — direct paths don't exist on web.".into(),
                );
            }
            ProjectCommand::Save { markers } => {
                let name = self.download_name(store);
                self.download_snapshot(store, name, markers);
            }
            ProjectCommand::SaveAs { .. } => {
                // Save destinations resolve through the saver picker first.
                store.state.status_msg.set(
                    "Use Save As to pick a file name, then download.".into(),
                );
            }
            ProjectCommand::Close => {
                self.editor = None;
                self.assets.clear();
                self.stop_playback(store);
                store.handle_event(AppEvent::ProjectClosed);
            }
        }
    }

    fn apply_asset(&mut self, store: &Store, cmd: snapshort_usecases::AssetCommand) {
        use snapshort_usecases::AssetCommand;
        match cmd {
            AssetCommand::Import { .. } => {
                store.state.status_msg.set(
                    "Media decoding needs the desktop backend — register files as pending instead."
                        .into(),
                );
            }
            AssetCommand::Delete { asset_id } => {
                self.assets.remove(&asset_id);
                store.handle_event(AppEvent::AssetDeleted { asset_id });
            }
            AssetCommand::GenerateProxy { .. } | AssetCommand::Analyze { .. } => {
                store.state.status_msg.set(
                    "Media analysis needs the desktop backend — register files as pending instead."
                        .into(),
                );
            }
            AssetCommand::UpdateMetadata { .. } => {
                store
                    .state
                    .status_msg
                    .set("Metadata editing needs the desktop backend.".into());
            }
        }
    }

    fn apply_playback(&mut self, store: &Store, cmd: snapshort_usecases::PlaybackCommand) {
        use snapshort_usecases::PlaybackCommand;
        match cmd {
            PlaybackCommand::Play => {
                if self.editor.is_none() {
                    store.state.status_msg.set("No project open".into());
                    return;
                }
                self.playing = true;
                self.last_tick = Some(web_time::Instant::now());
                store.handle_event(AppEvent::PlaybackStarted);
            }
            PlaybackCommand::Pause => {
                self.playing = false;
                store.handle_event(AppEvent::PlaybackPaused);
            }
            PlaybackCommand::Stop => {
                self.stop_playback(store);
                if let Some(editor) = self.editor.as_mut() {
                    editor.playhead = Timestamp::ZERO;
                }
                store.handle_event(AppEvent::PlayheadMoved {
                    timestamp: Timestamp::ZERO,
                });
            }
            PlaybackCommand::Seek { timestamp } => {
                let clamped = timestamp.clamp_non_negative();
                if let Some(editor) = self.editor.as_mut() {
                    editor.playhead = clamped;
                }
                store.handle_event(AppEvent::PlayheadMoved { timestamp: clamped });
            }
            PlaybackCommand::SetFps { .. } => {
                // Fixed 24 fps clock on web.
            }
        }
    }

    fn stop_playback(&mut self, store: &Store) {
        self.playing = false;
        self.last_tick = None;
        self.tick_acc_us = 0;
        store.handle_event(AppEvent::PlaybackStopped);
    }

    fn advance_playback(&mut self, store: &Store) {
        if !self.playing {
            return;
        }
        let now = web_time::Instant::now();
        let dt_us = self
            .last_tick
            .map(|t| now.duration_since(t).as_micros() as i64)
            .unwrap_or(0)
            .max(0);
        self.last_tick = Some(now);
        self.tick_acc_us += dt_us;

        let mut advanced = false;
        if let Some(editor) = self.editor.as_mut() {
            while self.tick_acc_us >= STEP_US {
                self.tick_acc_us -= STEP_US;
                editor.playhead = Timestamp(editor.playhead.0 + STEP_US);
                advanced = true;
            }
            let end_us = editor.project.timeline.duration_end().as_micros();
            if end_us > 0 && editor.playhead.0 >= end_us {
                editor.playhead = Timestamp(end_us);
                self.playing = false;
                self.tick_acc_us = 0;
                store.handle_event(AppEvent::PlaybackStopped);
            }
        }
        if advanced {
            if let Some(editor) = self.editor.as_ref() {
                store.handle_event(AppEvent::PlayheadMoved {
                    timestamp: editor.playhead,
                });
            }
        }
        // Keep the frame loop alive while playing (renamite wake pattern).
        repose_core::request_frame();
    }

    /// Load a project from uploaded `.snap` JSON bytes.
    pub fn open_json(&mut self, store: &Store, name: String, data: &[u8]) {
        let snapshot: ProjectSnapshot = match serde_json::from_slice(data) {
            Ok(s) => s,
            Err(e) => {
                store
                    .state
                    .status_msg
                    .set(format!("Could not parse {name}: {e}"));
                return;
            }
        };
        if snapshot.schema_version > ProjectSnapshot::SCHEMA_VERSION {
            store.state.status_msg.set(format!(
                "Unsupported project schema version: {}",
                snapshot.schema_version
            ));
            return;
        }
        let mut assets = HashMap::new();
        for asset in snapshot.assets {
            assets.insert(asset.id, asset);
        }
        self.assets = assets;
        let timeline = snapshot.project.timeline.clone();
        self.editor = Some(EditorState::new(snapshot.project.clone()));
        self.stop_playback(store);
        store.handle_event(AppEvent::ProjectOpened {
            project: snapshot.project,
            timeline_markers: snapshot.timeline_markers,
        });
        store.handle_event(AppEvent::TimelineUpdated { timeline });
        store.handle_event(AppEvent::AssetsLoaded {
            assets: self.assets.values().cloned().collect(),
        });
    }

    /// Register uploaded media without decoding (no decoders on web).
    pub fn ingest_media_bytes(&mut self, store: &Store, name: String, _data: Vec<u8>) {
        let lower = name.to_lowercase();
        let asset_type = if video_ext(&lower) {
            AssetType::Video
        } else if audio_ext(&lower) {
            AssetType::Audio
        } else if image_ext(&lower) {
            AssetType::Image
        } else {
            store
                .state
                .status_msg
                .set(format!("Unsupported file type: {name}"));
            return;
        };
        let asset = Asset::new(std::path::PathBuf::from(name.clone()), asset_type);
        let id = asset.id;
        self.assets.insert(id, asset.clone());
        store.handle_event(AppEvent::AssetImported { asset });
        store.state.status_msg.set(
            format!("{name} registered — decoding previews needs the desktop backend.").into(),
        );
    }

    pub fn serialize_snapshot(&self, markers: Vec<TimelineMarkerData>) -> Option<Vec<u8>> {
        let editor = self.editor.as_ref()?;
        let snapshot = ProjectSnapshot::new(
            editor.project.clone(),
            self.assets.values().cloned().collect(),
            markers,
        );
        serde_json::to_vec_pretty(&snapshot).ok()
    }

    fn download_name(&self, store: &Store) -> String {
        store
            .state
            .project
            .get()
            .map(|p| format!("{}.snap", p.meta.name))
            .unwrap_or_else(|| "project.snap".to_string())
    }

    fn download_snapshot(
        &self,
        store: &Store,
        name: String,
        markers: Vec<TimelineMarkerData>,
    ) {
        let Some(bytes) = self.serialize_snapshot(markers) else {
            store.state.status_msg.set("No project open".into());
            return;
        };
        let opts = rlobkit_dialogs::picker::SaveFileOptions {
            suggested_name: Some(name.clone()),
            extension: Some("snap".to_string()),
            title: Some("Save Project".to_string()),
            file_type: None,
            initial_directory: None,
            ..Default::default()
        };
        // Single-threaded wasm: the store can move into the local future.
        let ui = store.clone();
        let done_name = name.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match rlobkit_dialogs::RlobKit::save_bytes(opts, &bytes).await {
                Ok(_) => ui.state.status_msg.set(format!("Saved {done_name}")),
                Err(e) => ui.state.status_msg.set(format!("Save failed: {e}")),
            }
        });
        store
            .state
            .status_msg
            .set(format!("Downloading {name}…"));
    }

    /// Save/SaveAs entry points from picker outcomes.
    pub fn save_download(&self, store: &Store, name: Option<String>, markers: Vec<TimelineMarkerData>) {
        let name = name.unwrap_or_else(|| self.download_name(store));
        self.download_snapshot(store, name, markers);
    }
}
