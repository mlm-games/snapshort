//! The session: services wired together, one command loop.
//!
//! The same code serves the desktop app (a flume of UI commands), the CLI and
//! the MCP server (JSON commands), and the control channel (both). Event
//! orchestration lives here too, so a headless session reacts to imports,
//! analyses and proxy completions exactly like the app.
use crate::EngineError;
use crate::command::SessionCommand;
use crate::inspect;
use miniter_domain::Timestamp;
use miniter_usecases::Selection;
use serde_json::json;
use snapshort_infra_audio::AudioDspService;
use snapshort_infra_render::RenderService;
use snapshort_infra_store::{JobStore, ProjectStore};
use snapshort_usecases::{
    AUTOSAVE_INTERVAL_SECS, AppError, AppEvent, AppResult, AssetService, EventBus, JobsService,
    PlaybackCommand, PlaybackService, PreviewCommand, PreviewService, ProjectCommand,
    ProjectService, RenderCommand,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
/// A running export: the encoder polls the flag at its checkpoints, so
/// cancelling is prompt without killing the worker.
#[derive(Debug, Default)]
pub struct RunningExport {
    cancel: Arc<AtomicBool>,
}
pub struct Services {
    pub data_dir: PathBuf,
    pub project: Arc<ProjectService>,
    pub assets: Arc<AssetService>,
    pub jobs: Arc<JobsService>,
    pub playback: Arc<PlaybackService>,
    pub preview: Arc<PreviewService>,
    pub render: Arc<RenderService>,
    pub audio: Arc<AudioDspService>,
}
pub struct Session {
    services: Services,
    event_bus: EventBus,
    ui_tx: flume::Sender<AppEvent>,
    ui_rx: flume::Receiver<AppEvent>,
    export_state: Arc<Mutex<Option<RunningExport>>>,
    /// Outcome of the last export, for `render.export` with `wait: true`.
    last_render: Arc<Mutex<Option<serde_json::Value>>>,
    selection: Arc<Mutex<Selection>>,
    /// Timeline markers: part of the saved project, drawn on the ruler.
    markers: Arc<Mutex<Vec<snapshort_usecases::TimelineMarkerData>>>,
    /// The transcript: words in sequence time, from a recogniser or the audio's
    /// own speech runs. Cache for the dead-space and filler commands.
    transcript: Arc<Mutex<Vec<snapshort_edit::Word>>>,
    fps: Arc<AtomicI64>,
    started: AtomicBool,
}
impl Session {
    pub fn new(data_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&data_dir).ok();
        let proxy_dir = data_dir.join("proxies");
        std::fs::create_dir_all(&proxy_dir).ok();
        let event_bus = EventBus::new();
        let (ui_tx, ui_rx) = flume::unbounded::<AppEvent>();
        let _ = &ui_rx;
        let project_store = ProjectStore::new(&data_dir);
        let job_store = JobStore::new(&data_dir);
        let project = Arc::new(ProjectService::new(
            project_store,
            event_bus.clone(),
            data_dir.clone(),
        ));
        let jobs = Arc::new(JobsService::new(job_store, event_bus.clone(), proxy_dir));
        let assets = Arc::new(AssetService::new(event_bus.clone(), jobs.clone()));
        let playback = Arc::new(PlaybackService::new(event_bus.clone()));
        let render = Arc::new(RenderService::new());
        let preview = Arc::new(PreviewService::new(event_bus.clone(), render.clone()));
        let audio = Arc::new(AudioDspService::new());
        Self {
            services: Services {
                data_dir,
                project,
                assets,
                jobs,
                playback,
                preview,
                render,
                audio,
            },
            event_bus,
            ui_tx,
            ui_rx,
            export_state: Arc::new(Mutex::new(None)),
            last_render: Arc::new(Mutex::new(None)),
            selection: Arc::new(Mutex::new(Selection::default())),
            markers: Arc::new(Mutex::new(Vec::new())),
            transcript: Arc::new(Mutex::new(Vec::new())),
            fps: Arc::new(AtomicI64::new(24)),
            started: AtomicBool::new(false),
        }
    }
    pub fn services(&self) -> &Services {
        &self.services
    }
    pub fn data_dir(&self) -> &std::path::Path {
        &self.services.data_dir
    }
    pub fn fps(&self) -> i64 {
        self.fps.load(Ordering::SeqCst)
    }
    pub fn set_fps(&self, fps: i64) {
        self.fps.store(fps.max(1), Ordering::SeqCst);
    }
    /// Adopt a fractional frame rate for frame math and playback.
    pub async fn set_timeline_fps_from_preset(&self, fps: f64) {
        if fps > 0.0 {
            self.set_fps(fps.round() as i64);
        }
    }
    pub fn selection(&self) -> std::sync::MutexGuard<'_, Selection> {
        self.selection.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn markers(
        &self,
    ) -> std::sync::MutexGuard<'_, Vec<snapshort_usecases::TimelineMarkerData>> {
        self.markers.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Markers changed: the UI mirrors this onto the ruler.
    pub async fn transcript(&self) -> Vec<snapshort_edit::Word> {
        self.transcript.lock().unwrap().clone()
    }
    pub async fn set_transcript(&self, words: Vec<snapshort_edit::Word>) {
        *self.transcript.lock().unwrap() = words;
    }
    pub async fn note_markers_changed(&self) {
        self.emit_markers_changed().await;
    }
    async fn emit_markers_changed(&self) {
        let markers = self.markers().clone();
        self.event_bus.emit(AppEvent::MarkersChanged { markers });
    }
    pub fn export_in_flight(&self) -> bool {
        self.export_state
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(true)
    }
    /// Events after orchestration: the UI pump drains these each frame.
    pub fn events(&self) -> flume::Receiver<AppEvent> {
        self.ui_rx.clone()
    }
    /// Build the stores, start the orchestration and crash-recovery loops, and
    /// boot a project (or stage the recovery offer). Idempotent.
    pub async fn start(&self) -> AppResult<()> {
        if self.started.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        if let Err(e) = self.services.jobs.recover_and_resume().await {
            tracing::warn!("Job recovery failed, pending jobs may be stuck: {e}");
        }
        self.spawn_orchestration();
        self.spawn_autosave();
        match self.services.project.check_autosave().await {
            Some(found) => {
                let _ = self.ui_tx.send(AppEvent::AutosaveFound {
                    project_name: found.project_name,
                    saved_at_ms: found.saved_at_ms,
                });
            }
            None => {
                self.services
                    .project
                    .execute(ProjectCommand::Create {
                        name: "Untitled".to_string(),
                    })
                    .await?;
            }
        }
        Ok(())
    }
    /// Forward bus events to the UI and apply the orchestration the app's
    /// backend loop used to own (asset wiring, preview paths, playback gating).
    fn spawn_orchestration(&self) {
        let rx = self.event_bus.receiver();
        let ui_tx = self.ui_tx.clone();
        let project = self.services.project.clone();
        let assets = self.services.assets.clone();
        let jobs = self.services.jobs.clone();
        let preview = self.services.preview.clone();
        let playback = self.services.playback.clone();
        let fps = self.fps.clone();
        let markers_cell = self.markers.clone();
        tokio::spawn(async move {
            while let Ok(ev) = rx.recv_async().await {
                // Markers travel with the project.
                if let AppEvent::ProjectOpened {
                    timeline_markers, ..
                } = &ev
                {
                    *markers_cell.lock().unwrap_or_else(|e| e.into_inner()) =
                        timeline_markers.clone();
                }
                if matches!(
                    ev,
                    AppEvent::ProjectCreated { .. } | AppEvent::ProjectClosed
                ) {
                    markers_cell
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clear();
                }
                if let AppEvent::ProjectCreated { project: p }
                | AppEvent::ProjectOpened { project: p, .. } = &ev
                {
                    let loaded = project.list_assets().await;
                    assets.load_assets(loaded.clone()).await;
                    let offline = assets.mark_missing_offline().await;
                    jobs.load_assets(offline.clone()).await;
                    let path_map: HashMap<_, _> = offline
                        .iter()
                        .map(|a| (a.id, a.effective_path().clone()))
                        .collect();
                    preview.update_asset_paths(path_map).await;
                    if let Some(video) = p
                        .timeline
                        .tracks
                        .iter()
                        .flat_map(|t| &t.clips)
                        .find_map(|c| match &c.kind {
                            miniter_domain::ClipKind::Video(v) => Some(v.fps),
                            _ => None,
                        })
                        .filter(|f| *f > 0.0)
                    {
                        fps.store(video.round() as i64, Ordering::SeqCst);
                    }
                }
                if let AppEvent::TimelineUpdated { timeline } = &ev {
                    preview.update_timeline(Some(timeline.clone())).await;
                    let end = timeline.duration_end();
                    if end.0 > 0 {
                        playback.set_max_timestamp(Some(end)).await;
                    }
                }
                if let AppEvent::ProjectClosed = &ev {
                    preview.update_timeline(None).await;
                    preview.update_asset_paths(HashMap::new()).await;
                }
                if matches!(ev, AppEvent::PlaybackStarted) {
                    preview.set_playing(true).await;
                }
                if matches!(ev, AppEvent::PlaybackPaused | AppEvent::PlaybackStopped) {
                    preview.set_playing(false).await;
                }
                if let AppEvent::AssetImported { asset }
                | AppEvent::AssetUpdated { asset }
                | AppEvent::AssetAnalyzed { asset }
                | AppEvent::AssetProxyComplete { asset } = &ev
                {
                    preview
                        .upsert_asset_path(asset.id, asset.effective_path().clone())
                        .await;
                    project.add_asset(asset.clone()).await;
                    if matches!(ev, AppEvent::AssetAnalyzed { .. }) {
                        assets.note_analyzed(asset.clone()).await;
                    }
                }
                if let AppEvent::AssetProxyComplete { asset } = &ev
                    && let Some(proxy) = &asset.proxy
                {
                    preview
                        .set_proxy(asset.path.clone(), proxy.path.clone())
                        .await;
                }
                if let AppEvent::AssetDeleted { asset_id } = &ev
                    && let Some(asset) = project.remove_asset(*asset_id).await
                {
                    preview.remove_asset_path(*asset_id).await;
                    preview.clear_proxy_for_source(&asset.path).await;
                }
                let _ = ui_tx.send(ev);
            }
        });
    }
    fn spawn_autosave(&self) {
        let svc = self.services.project.clone();
        let dir = self.services.data_dir.clone();
        tokio::spawn(async move {
            let mut tick =
                tokio::time::interval(web_time::Duration::from_secs(AUTOSAVE_INTERVAL_SECS));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let Some((path, snapshot)) = svc.autosave_snapshot().await else {
                    continue;
                };
                let meta = snapshort_usecases::AutosaveMeta {
                    project_path: path,
                    project_name: snapshot.project.meta.name.clone(),
                    saved_at_ms: snapshort_usecases::now_ms(),
                };
                if let Err(e) = snapshort_usecases::write_autosave(&dir, &snapshot, &meta) {
                    tracing::warn!("Autosave failed: {e}");
                }
                // A rolling set of versions, so a bad edit an hour ago is still
                // recoverable; the newest `AUTOSAVE_VERSIONS` are kept.
                let stamp = snapshort_usecases::now_ms();
                let version = snapshort_usecases::autosave_version_path(&dir, stamp);
                if let Err(e) = snapshort_usecases::write_autosave_at(&version, &snapshot, &meta) {
                    tracing::warn!("Autosave version failed: {e}");
                }
                snapshort_usecases::prune_autosave_versions(&dir);
            }
        });
    }
    /// The rolling versions available to recover.
    pub fn autosave_versions(&self) -> Vec<serde_json::Value> {
        snapshort_usecases::list_autosave_versions(self.data_dir())
            .into_iter()
            .map(|version| {
                json!({
                    "index": version.index,
                    "savedAtMs": version.saved_at_ms,
                    "projectName": version.project_name,
                    "projectPath": version.project_path.map(|p| p.display().to_string()),
                })
            })
            .collect()
    }
    /// Restore one of the rolling versions (default: the newest).
    pub async fn restore_autosave_version(&self, index: Option<usize>) -> Result<(), EngineError> {
        let versions = snapshort_usecases::list_autosave_versions(self.data_dir());
        let Some(version) = versions
            .get(index.unwrap_or(0))
            .or_else(|| versions.first())
            .cloned()
        else {
            return Err(EngineError::Other("there are no saved versions".into()));
        };
        let snapshot = snapshort_usecases::read_snapshot(&version.path)
            .map_err(|e| EngineError::Other(format!("cannot read that version: {e}")))?;
        self.services
            .project
            .execute(ProjectCommand::Open {
                path: version.path.clone(),
            })
            .await?;
        *self.markers() = snapshot.timeline_markers;
        self.emit_markers_changed().await;
        Ok(())
    }
    /// Serve a flume of UI commands until the channel closes. Failures reach
    /// the UI as error events instead of stopping the loop.
    pub async fn serve(&self, cmd_rx: flume::Receiver<SessionCommand>) {
        while let Ok(cmd) = cmd_rx.recv_async().await {
            if let Err(e) = self.dispatch(cmd).await {
                self.event_bus.emit(AppEvent::Error {
                    message: e.to_string(),
                });
            }
        }
    }
    /// Apply one typed command and report what it did.
    pub async fn dispatch(&self, cmd: SessionCommand) -> AppResult<serde_json::Value> {
        let s = &self.services;
        match cmd {
            SessionCommand::Project(c) => {
                // A save with no markers of its own carries the session's.
                let command = match c {
                    ProjectCommand::Save { markers } if markers.is_empty() => {
                        ProjectCommand::Save {
                            markers: self.markers().clone(),
                        }
                    }
                    ProjectCommand::SaveAs { path, markers } if markers.is_empty() => {
                        ProjectCommand::SaveAs {
                            path,
                            markers: self.markers().clone(),
                        }
                    }
                    other => other,
                };
                match &command {
                    ProjectCommand::Open { path } => {
                        // Read the markers back with the project, so callers
                        // see them the moment Open returns.
                        let markers = snapshort_usecases::read_snapshot(path)
                            .map(|snapshot| snapshot.timeline_markers)
                            .unwrap_or_default();
                        *self.markers() = markers;
                    }
                    ProjectCommand::Create { .. } | ProjectCommand::Close => {
                        self.markers().clear();
                    }
                    _ => {}
                }
                s.project.execute(command).await?;
                self.emit_markers_changed().await;
                if s.project.current_project().await.is_some() {
                    Ok(inspect::project_json(self).await)
                } else {
                    Ok(serde_json::json!({"ok": true, "open": false}))
                }
            }
            SessionCommand::Edit { cmd, label } => {
                s.project.dispatch_timeline_command(*cmd, label).await?;
                Ok(inspect::timeline_json(self).await)
            }
            SessionCommand::Undo => {
                s.project.undo_timeline().await?;
                Ok(self.undo_state_json().await)
            }
            SessionCommand::Redo => {
                s.project.redo_timeline().await?;
                Ok(self.undo_state_json().await)
            }
            SessionCommand::Asset(c) => {
                s.assets.execute(c).await?;
                Ok(inspect::assets_json(self).await)
            }
            SessionCommand::Playback(c) => match c {
                PlaybackCommand::Play => {
                    s.playback.play().await;
                    Ok(
                        serde_json::json!({"ok": true, "state": format!("{:?}", s.playback.state().await)}),
                    )
                }
                PlaybackCommand::Pause => {
                    s.playback.pause().await;
                    Ok(serde_json::json!({"ok": true}))
                }
                PlaybackCommand::Stop => {
                    s.playback.stop().await;
                    Ok(serde_json::json!({"ok": true}))
                }
                PlaybackCommand::Seek { timestamp } => {
                    s.playback.seek(timestamp).await;
                    s.project.set_playhead(timestamp).await;
                    Ok(serde_json::json!({"ok": true, "playhead": timestamp.0}))
                }
                PlaybackCommand::SetFps { fps } => {
                    s.playback.set_fps(fps).await;
                    self.set_fps(fps);
                    Ok(serde_json::json!({"ok": true, "fps": fps}))
                }
            },
            SessionCommand::Preview(c) => match c {
                PreviewCommand::RequestFrame { timestamp } => {
                    s.preview.request_frame(timestamp).await;
                    Ok(serde_json::json!({"ok": true}))
                }
                PreviewCommand::RequestTimelineThumbnail {
                    asset_id,
                    source_time,
                } => {
                    s.preview
                        .request_timeline_thumbnail(asset_id, source_time)
                        .await;
                    Ok(serde_json::json!({"ok": true}))
                }
                PreviewCommand::SetPreferProxy { prefer } => {
                    s.preview.set_prefer_proxy(prefer).await;
                    Ok(serde_json::json!({"ok": true}))
                }
            },
            SessionCommand::RunJson { id, params } => {
                let outcome = self.execute(&id, params).await;
                let message = match &outcome {
                    Ok(_) => format!("{id}: ok"),
                    Err(e) => format!("{id} failed: {e}"),
                };
                Ok(serde_json::json!({"ok": outcome.is_ok(), "message": message}))
            }
            SessionCommand::Select { clips, tracks } => {
                {
                    let mut selection = self.selection();
                    if clips.is_empty() && tracks.is_empty() {
                        selection.clear();
                    } else {
                        selection.clips = clips;
                        selection.tracks = tracks;
                    }
                }
                Ok(crate::inspect::selection_json(self).await)
            }
            SessionCommand::Render(c) => self.dispatch_render(c).await,
        }
    }
    async fn dispatch_render(&self, c: RenderCommand) -> AppResult<serde_json::Value> {
        let s = &self.services;
        match c {
            RenderCommand::PreparePlan => {
                let Some(timeline) = s.project.current_timeline().await else {
                    return Err(AppError::Other("No active timeline to render".into()));
                };
                let settings = s.render.recommended_settings(&timeline);
                let plan = s.render.build_render_plan(&timeline, settings);
                self.event_bus
                    .emit(AppEvent::RenderPlanReady { plan: plan.clone() });
                Ok(serde_json::json!({
                    "totalFrames": plan.total_frames,
                    "durationUs": plan.duration_us,
                    "maxLayers": plan.max_layers,
                    "issues": plan.issues,
                    "settings": {
                        "outputPath": plan.settings.output_path.display().to_string(),
                        "resolution": [plan.settings.resolution.0, plan.settings.resolution.1],
                        "fps": plan.settings.fps,
                        "videoBitrate": plan.settings.video_bitrate,
                        "audioBitrate": plan.settings.audio_bitrate,
                        "frameRange": plan.settings.frame_range,
                        "hardwareAccel": plan.settings.use_hardware_accel,
                    },
                }))
            }
            RenderCommand::CancelExport => {
                let running = self
                    .export_state
                    .lock()
                    .ok()
                    .and_then(|g| g.as_ref().map(|r| r.cancel.clone()));
                if let Some(cancel) = running {
                    cancel.store(true, Ordering::SeqCst);
                    Ok(serde_json::json!({"ok": true}))
                } else {
                    Err(AppError::Other("No export is running".into()))
                }
            }
            RenderCommand::Export {
                output_path,
                format,
                quality,
                use_hardware_accel,
                track_volumes,
                master_volume,
            } => {
                if self.export_in_flight() {
                    return Err(AppError::Other("Export already in progress".into()));
                }
                let Some(timeline) = s.project.current_timeline().await else {
                    return Err(AppError::Other("No active timeline to render".into()));
                };
                let missing = snapshort_usecases::missing_timeline_sources(&timeline);
                if !missing.is_empty() {
                    let names: Vec<String> = missing
                        .iter()
                        .take(3)
                        .map(|p| {
                            p.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("?")
                                .to_string()
                        })
                        .collect();
                    return Err(AppError::Other(format!(
                        "{} media file(s) offline ({}…). Relink them, then export again.",
                        missing.len(),
                        names.join(", ")
                    )));
                }
                let mut settings = s.render.recommended_settings(&timeline);
                settings.output_path = output_path;
                settings.format = format;
                settings.quality = quality;
                settings.use_hardware_accel =
                    use_hardware_accel && s.render.is_hardware_accel_available();
                self.event_bus.emit(AppEvent::RenderStarted {
                    settings: settings.clone(),
                });
                let cancel = Arc::new(AtomicBool::new(false));
                if let Ok(mut slot) = self.export_state.lock() {
                    *slot = Some(RunningExport {
                        cancel: cancel.clone(),
                    });
                }
                let bus = self.event_bus.clone();
                let progress_bus = bus.clone();
                let render = s.render.clone();
                let slot = self.export_state.clone();
                let outcome = self.last_render.clone();
                let cancel_flag = cancel.clone();
                tokio::spawn(async move {
                    let gate = snapshort_infra_render::ProgressGate::new();
                    let result = tokio::task::spawn_blocking(move || {
                        render.export_timeline(
                            &timeline,
                            &settings,
                            &track_volumes,
                            master_volume,
                            &|| cancel_flag.load(Ordering::SeqCst),
                            &|pct| {
                                if gate.check(pct, snapshort_infra_render::ProgressGate::now_ms()) {
                                    progress_bus.emit(AppEvent::RenderProgress {
                                        percent: pct.min(100),
                                    });
                                }
                            },
                        )
                    })
                    .await;
                    if let Ok(mut slot) = slot.lock() {
                        *slot = None;
                    }
                    match result {
                        Ok(Ok(result)) => {
                            let report = serde_json::json!({
                                "ok": true,
                                "output": result.output_path.display().to_string(),
                                "seconds": result.render_time_seconds,
                                "bytes": result.file_size_bytes,
                            });
                            if let Ok(mut slot) = outcome.lock() {
                                *slot = Some(report);
                            }
                            bus.emit(AppEvent::RenderFinished { result });
                        }
                        Ok(Err(err)) => {
                            let report = serde_json::json!({"ok": false, "error": err.to_string()});
                            if let Ok(mut slot) = outcome.lock() {
                                *slot = Some(report);
                            }
                            bus.emit(AppEvent::RenderFailed {
                                error: err.to_string(),
                            });
                        }
                        Err(join_err) => {
                            let error = format!("Export task failed: {join_err}");
                            if let Ok(mut slot) = outcome.lock() {
                                *slot = Some(serde_json::json!({"ok": false, "error": error}));
                            }
                            bus.emit(AppEvent::RenderFailed { error });
                        }
                    }
                });
                Ok(serde_json::json!({"ok": true, "started": true}))
            }
        }
    }
    async fn undo_state_json(&self) -> serde_json::Value {
        let (can_undo, can_redo, undo_label, redo_label) = {
            let guard = self.services.project.editor().await;
            let guard = guard.read().await;
            match guard.as_ref() {
                Some(editor) => (
                    editor.history.can_undo(),
                    editor.history.can_redo(),
                    editor.history.undo_label().map(str::to_string),
                    editor.history.redo_label().map(str::to_string),
                ),
                None => (false, false, None, None),
            }
        };
        serde_json::json!({
            "ok": true,
            "canUndo": can_undo,
            "canRedo": can_redo,
            "undoLabel": undo_label,
            "redoLabel": redo_label,
        })
    }
    /// Run a JSON command: the single path agents, scripts and the CLI use.
    pub async fn execute(
        &self,
        id: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, EngineError> {
        let Some(spec) = crate::commands::find(id) else {
            return Err(EngineError::UnknownCommand(id.to_string()));
        };
        if let Err(reason) = (spec.enabled)(self).await {
            return Err(EngineError::Disabled(id.to_string(), reason));
        }
        (spec.run)(self, &params).await
    }
    /// The active frame as PNG bytes, downscaled so the longest side is at
    /// most `max_side` (0 = no limit).
    pub async fn render_frame_png(
        &self,
        at: Option<Timestamp>,
        max_side: u32,
    ) -> Result<Vec<u8>, EngineError> {
        let Some(timeline) = self.services.project.current_timeline().await else {
            return Err(EngineError::Other("no project is open".into()));
        };
        let at = at
            .unwrap_or_else(|| Timestamp::from_micros(0))
            .max(Timestamp::ZERO);
        let render = self.services.render.clone();
        let png = tokio::task::spawn_blocking(move || render.render_preview_frame(&timeline, at))
            .await
            .map_err(|e| EngineError::Other(format!("render task failed: {e}")))?
            .map_err(|e| EngineError::Other(e.to_string()))?;
        scale_png(&png, max_side)
    }
    /// Wait until an export finishes (or fails), returning its result.
    pub async fn await_export(
        &self,
        timeout: std::time::Duration,
    ) -> Result<serde_json::Value, EngineError> {
        let deadline = web_time::Instant::now() + timeout;
        loop {
            if !self.export_in_flight() {
                break;
            }
            if web_time::Instant::now() >= deadline {
                return Err(EngineError::Other("export still running".into()));
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        Ok(serde_json::json!({"ok": true}))
    }
    pub fn set_timeline_fps_from_clips(&self, timeline: &miniter_domain::Timeline) {
        self.note_timeline_fps_from(timeline);
    }
    fn note_timeline_fps_from(&self, timeline: &miniter_domain::Timeline) {
        if let Some(fps) =
            timeline
                .tracks
                .iter()
                .flat_map(|t| &t.clips)
                .find_map(|c| match &c.kind {
                    miniter_domain::ClipKind::Video(v) if v.fps > 0.0 => Some(v.fps),
                    _ => None,
                })
        {
            self.set_fps(fps.round() as i64);
        }
    }
    /// Adopt the frame rate of the timeline's first video clip (if any).
    pub async fn note_timeline_fps(&self) {
        if let Some(timeline) = self.services.project.current_timeline().await {
            self.note_timeline_fps_from(&timeline);
        }
    }
    /// Wait for a render to finish, then report how it went.
    pub async fn await_render(&self, timeout: u64) -> Result<serde_json::Value, EngineError> {
        let deadline = web_time::Instant::now() + web_time::Duration::from_secs(timeout);
        while self.export_in_flight() {
            if web_time::Instant::now() >= deadline {
                return Err(EngineError::Other("export still running".into()));
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        Ok(self
            .last_render
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
            .unwrap_or_else(|| serde_json::json!({"ok": true})))
    }
}
/// Downscale a PNG so its longest side is at most `max_side`.
pub fn scale_png(png: &[u8], max_side: u32) -> Result<Vec<u8>, EngineError> {
    if max_side == 0 {
        return Ok(png.to_vec());
    }
    let img = image::load_from_memory(png).map_err(|e| EngineError::Other(e.to_string()))?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    if w.max(h) <= max_side {
        return Ok(png.to_vec());
    }
    let scale = max_side as f32 / w.max(h) as f32;
    let resized = image::imageops::resize(
        &rgba,
        ((w as f32 * scale) as u32).max(1),
        ((h as f32 * scale) as u32).max(1),
        image::imageops::FilterType::Triangle,
    );
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(resized)
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| EngineError::Other(e.to_string()))?;
    Ok(out.into_inner())
}
