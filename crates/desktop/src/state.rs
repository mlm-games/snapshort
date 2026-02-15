use flume::Sender;
use repose_core::request_frame;
use repose_core::signal::signal;
use repose_docking::DockState;
use repose_platform::RenderContext;
use snapshort_domain::prelude::*;
use snapshort_infra_render::{OutputFormat, QualityPreset};
use snapshort_usecases::{
    AppEvent, AssetCommand, PlaybackCommand, ProjectCommand, RenderCommand, TimelineCommand,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};

/// Content stored in the clipboard for copy/cut/paste operations
#[derive(Debug, Clone)]
pub struct ClipboardContent {
    /// The copied clip data
    pub clip: Clip,
    /// Whether this was a cut operation (clip should be removed on paste)
    pub is_cut: bool,
}

/// The single source of truth for the UI, using Repose signals
#[derive(Clone)]
pub struct AppState {
    pub project: repose_core::signal::Signal<Option<Project>>,
    pub assets: repose_core::signal::Signal<Vec<Asset>>,
    pub timeline: repose_core::signal::Signal<Option<Timeline>>,
    pub status_msg: repose_core::signal::Signal<String>,
    pub is_loading: repose_core::signal::Signal<bool>,

    // Playback + error state
    pub playback_state: repose_core::signal::Signal<String>,
    pub last_error: repose_core::signal::Signal<Option<String>>,

    // Selection
    pub selected_asset_id: repose_core::signal::Signal<Option<AssetId>>,
    pub selected_clip_id: repose_core::signal::Signal<Option<ClipId>>,

    // Timeline zoom (pixels per frame)
    pub timeline_zoom: repose_core::signal::Signal<f32>,

    // Last generated render plan (debug/mvp)
    pub last_render_plan_summary: repose_core::signal::Signal<Option<String>>,

    // Export settings (MVP)
    pub export_output_path: repose_core::signal::Signal<Option<PathBuf>>,
    pub export_format: repose_core::signal::Signal<OutputFormat>,
    pub export_quality: repose_core::signal::Signal<QualityPreset>,
    pub export_use_hw_accel: repose_core::signal::Signal<bool>,
    pub last_render_result: repose_core::signal::Signal<Option<String>>,

    // Preview image handle for program monitor
    pub preview_image_handle: repose_core::signal::Signal<repose_core::ImageHandle>,
}

pub struct Store {
    pub state: AppState,
    cmd_tx: Sender<BackendCommand>,
    /// Clipboard for copy/cut/paste operations
    clipboard: RefCell<Option<ClipboardContent>>,
    /// Docking layout state
    pub dock_state: Rc<RefCell<DockState>>,
    pub preview_last_key: RefCell<Option<(AssetId, i64)>>,
    pub preview_in_flight: std::sync::Arc<AtomicBool>,
    pub preview_generation: Arc<AtomicU64>,
    pub render_ctx: RefCell<Option<RenderContext>>,
    pub timeline_thumb_cache: Arc<Mutex<HashMap<(AssetId, i64), repose_core::ImageHandle>>>,
    pub timeline_thumb_in_flight: Arc<Mutex<HashSet<(AssetId, i64)>>>,
}

impl Clone for Store {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            cmd_tx: self.cmd_tx.clone(),
            clipboard: RefCell::new(self.clipboard.borrow().clone()),
            dock_state: self.dock_state.clone(),
            preview_last_key: RefCell::new(*self.preview_last_key.borrow()),
            preview_in_flight: self.preview_in_flight.clone(),
            preview_generation: self.preview_generation.clone(),
            render_ctx: RefCell::new(self.render_ctx.borrow().clone()),
            timeline_thumb_cache: self.timeline_thumb_cache.clone(),
            timeline_thumb_in_flight: self.timeline_thumb_in_flight.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum BackendCommand {
    Project(ProjectCommand),
    Timeline(TimelineCommand),
    Asset(AssetCommand),
    Playback(PlaybackCommand),
    Render(RenderCommand),
}

impl Store {
    pub fn new(cmd_tx: Sender<BackendCommand>, dock_state: DockState) -> Self {
        Self {
            state: AppState {
                project: signal(None),
                assets: signal(vec![]),
                timeline: signal(None),
                status_msg: signal("Ready".to_string()),
                is_loading: signal(false),
                playback_state: signal("Stopped".to_string()),
                last_error: signal(None),
                selected_asset_id: signal(None),
                selected_clip_id: signal(None),
                timeline_zoom: signal(2.0),
                last_render_plan_summary: signal(None),
                export_output_path: signal(None),
                export_format: signal(OutputFormat::Mp4H264),
                export_quality: signal(QualityPreset::Standard),
                export_use_hw_accel: signal(false),
                last_render_result: signal(None),
                preview_image_handle: signal(0),
            },
            cmd_tx,
            clipboard: RefCell::new(None),
            dock_state: Rc::new(RefCell::new(dock_state)),
            preview_last_key: RefCell::new(None),
            preview_in_flight: std::sync::Arc::new(AtomicBool::new(false)),
            preview_generation: Arc::new(AtomicU64::new(0)),
            render_ctx: RefCell::new(None),
            timeline_thumb_cache: Arc::new(Mutex::new(HashMap::new())),
            timeline_thumb_in_flight: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn ensure_render_context(&self, rc: &RenderContext) {
        if self.render_ctx.borrow().is_some() {
            return;
        }
        let handle = rc.alloc_image_handle();
        self.state.preview_image_handle.set(handle);
        rc.set_image_rgba8(handle, 1, 1, vec![0, 0, 0, 255], true);
        *self.render_ctx.borrow_mut() = Some(rc.clone());
    }

    pub fn dispatch_project(&self, cmd: ProjectCommand) {
        let _ = self.cmd_tx.send(BackendCommand::Project(cmd));
    }
    pub fn dispatch_timeline(&self, cmd: TimelineCommand) {
        let _ = self.cmd_tx.send(BackendCommand::Timeline(cmd));
    }
    pub fn dispatch_asset(&self, cmd: AssetCommand) {
        let _ = self.cmd_tx.send(BackendCommand::Asset(cmd));
    }
    pub fn dispatch_playback(&self, cmd: PlaybackCommand) {
        let _ = self.cmd_tx.send(BackendCommand::Playback(cmd));
    }
    pub fn dispatch_render(&self, cmd: RenderCommand) {
        let _ = self.cmd_tx.send(BackendCommand::Render(cmd));
    }

    /// Copy the currently selected clip to the clipboard
    pub fn copy_selected_clip(&self) {
        if let Some(clip_id) = self.state.selected_clip_id.get() {
            if let Some(timeline) = self.state.timeline.get() {
                if let Some(clip) = timeline.clips.iter().find(|c| c.id == clip_id) {
                    *self.clipboard.borrow_mut() = Some(ClipboardContent {
                        clip: clip.clone(),
                        is_cut: false,
                    });
                    self.state.status_msg.set("Clip copied".into());
                }
            }
        } else {
            self.state.status_msg.set("No clip selected".into());
        }
    }

    /// Cut the currently selected clip to the clipboard
    pub fn cut_selected_clip(&self) {
        if let Some(clip_id) = self.state.selected_clip_id.get() {
            if let Some(timeline) = self.state.timeline.get() {
                if let Some(clip) = timeline.clips.iter().find(|c| c.id == clip_id) {
                    *self.clipboard.borrow_mut() = Some(ClipboardContent {
                        clip: clip.clone(),
                        is_cut: true,
                    });
                    // Remove the original clip
                    self.dispatch_timeline(TimelineCommand::RemoveClip { clip_id });
                    self.state.selected_clip_id.set(None);
                    self.state.status_msg.set("Clip cut".into());
                }
            }
        } else {
            self.state.status_msg.set("No clip selected".into());
        }
    }

    /// Paste the clip from clipboard at the current playhead position
    pub fn paste_clip(&self) {
        let clipboard = self.clipboard.borrow();
        if let Some(content) = clipboard.as_ref() {
            if let Some(timeline) = self.state.timeline.get() {
                // Paste at playhead position on the same track
                if let Some(asset_id) = content.clip.asset_id {
                    self.dispatch_timeline(TimelineCommand::InsertClip {
                        asset_id,
                        timeline_start: timeline.playhead,
                        track: content.clip.track.clone(),
                        source_range: Some(content.clip.source_range.clone()),
                    });
                    self.state.status_msg.set("Clip pasted".into());
                } else {
                    self.state.status_msg.set("Cannot paste gap clips".into());
                }
            }
        } else {
            self.state.status_msg.set("Clipboard is empty".into());
        }
    }

    pub fn handle_event(&self, event: AppEvent) {
        match event {
            AppEvent::ProjectCreated { project } => {
                self.state.project.set(Some(project));
                self.state.status_msg.set("Project initialized".into());
            }
            AppEvent::ProjectOpened { project } => {
                self.state.project.set(Some(project));
                self.state.status_msg.set("Project opened".into());
            }
            AppEvent::ProjectSaved { .. } => {
                self.state.status_msg.set("Project saved".into());
            }
            AppEvent::ProjectClosed => {
                self.state.project.set(None);
                self.state.timeline.set(None);
                self.state.assets.set(vec![]);
                self.state.selected_asset_id.set(None);
                self.state.selected_clip_id.set(None);
                self.state.status_msg.set("Project closed".into());
            }

            AppEvent::TimelineCreated { timeline } | AppEvent::TimelineUpdated { timeline } => {
                self.state.timeline.set(Some(timeline));
            }
            AppEvent::ActiveTimelineChanged { .. } => {}

            AppEvent::PlayheadMoved { frame } => {
                if let Some(mut tl) = self.state.timeline.get() {
                    tl.playhead = frame;
                    self.state.timeline.set(Some(tl));
                    request_frame();
                }
            }

            AppEvent::PlaybackStarted => {
                self.state.playback_state.set("Playing".into());
                self.state.status_msg.set("Playing".into());
            }
            AppEvent::PlaybackPaused => {
                self.state.playback_state.set("Paused".into());
                self.state.status_msg.set("Paused".into());
            }
            AppEvent::PlaybackStopped => {
                self.state.playback_state.set("Stopped".into());
                self.state.status_msg.set("Stopped".into());
            }

            AppEvent::RenderPlanReady {
                timeline_id: _,
                plan,
            } => {
                self.state.last_render_plan_summary.set(Some(format!(
                    "Render plan ready: {} clips",
                    plan.clips.len()
                )));
                self.state.status_msg.set("Render plan ready".into());
            }
            AppEvent::RenderStarted { settings } => {
                self.state
                    .status_msg
                    .set(format!("Exporting to {}…", settings.output_path.display()));
                self.state.last_render_result.set(None);
            }
            AppEvent::RenderFinished { result } => {
                self.state.status_msg.set("Export complete".into());
                self.state.last_render_result.set(Some(format!(
                    "Exported to {}",
                    result.output_path.display()
                )));
            }
            AppEvent::RenderFailed { error } => {
                self.state.status_msg.set("Export failed".into());
                self.state
                    .last_render_result
                    .set(Some(format!("Export failed: {error}")));
            }

            AppEvent::AssetsLoaded { assets } => {
                self.state.assets.set(assets);
            }

            AppEvent::AssetImported { asset } => {
                let mut list = self.state.assets.get();
                list.push(asset);
                self.state.assets.set(list);
            }
            AppEvent::AssetUpdated { asset }
            | AppEvent::AssetAnalyzed { asset }
            | AppEvent::AssetProxyComplete { asset } => {
                let mut list = self.state.assets.get();
                if let Some(i) = list.iter().position(|a| a.id == asset.id) {
                    list[i] = asset;
                    self.state.assets.set(list);
                }
            }
            AppEvent::AssetDeleted { asset_id } => {
                let mut list = self.state.assets.get();
                list.retain(|a| a.id != asset_id);
                self.state.assets.set(list);

                if self.state.selected_asset_id.get() == Some(asset_id) {
                    self.state.selected_asset_id.set(None);
                }
            }
            AppEvent::AssetProxyProgress { asset_id, progress } => {
                let mut list = self.state.assets.get();
                if let Some(i) = list.iter().position(|a| a.id == asset_id) {
                    let mut a = list[i].clone();
                    a.status = AssetStatus::ProxyGenerating { progress };
                    list[i] = a;
                    self.state.assets.set(list);
                }
            }

            // Phase 1 jobs events: loading state toggle
            AppEvent::JobQueued { .. }
            | AppEvent::JobStarted { .. }
            | AppEvent::JobProgress { .. } => {
                self.state.is_loading.set(true);
            }
            AppEvent::JobFinished { .. }
            | AppEvent::JobFailed { .. }
            | AppEvent::JobCanceled { .. } => {
                self.state.is_loading.set(false);
            }

            AppEvent::UndoStackChanged { .. } => {}

            AppEvent::Error { message } => {
                self.state.last_error.set(Some(message.clone()));
                self.state.status_msg.set(format!("Error: {}", message));
            }
        }
    }
}
