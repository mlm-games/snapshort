use flume::Sender;
use miniter_domain::{Clip, ClipId, Timestamp};
use miniter_usecases::EditCommand;
use repose_core::request_frame;
use repose_core::signal::signal;
use repose_docking::DockState;
use repose_platform::RenderContext;
use snapshort_infra_render::QualityPreset;
use snapshort_usecases::{
    AppEvent, Asset, AssetCommand, AssetId, AssetStatus, PlaybackCommand, PreviewCommand,
    ProjectCommand, RenderCommand,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct ClipboardContent {
    pub clip: Clip,
    pub is_cut: bool,
}

#[derive(Clone)]
pub struct AppState {
    pub project: repose_core::signal::Signal<Option<miniter_domain::Project>>,
    pub assets: repose_core::signal::Signal<Vec<Asset>>,
    pub timeline: repose_core::signal::Signal<Option<miniter_domain::Timeline>>,
    pub status_msg: repose_core::signal::Signal<String>,
    pub is_loading: repose_core::signal::Signal<bool>,
    pub playback_state: repose_core::signal::Signal<String>,
    pub last_error: repose_core::signal::Signal<Option<String>>,
    pub selected_asset_id: repose_core::signal::Signal<Option<AssetId>>,
    pub selected_clip_id: repose_core::signal::Signal<Option<ClipId>>,
    pub timeline_zoom: repose_core::signal::Signal<f32>,
    pub timeline_snap: repose_core::signal::Signal<bool>,
    pub last_render_plan_summary: repose_core::signal::Signal<Option<String>>,
    pub export_output_path: repose_core::signal::Signal<Option<PathBuf>>,
    pub export_quality: repose_core::signal::Signal<QualityPreset>,
    pub last_render_result: repose_core::signal::Signal<Option<String>>,
    pub preview_image_handle: repose_core::signal::Signal<repose_core::ImageHandle>,
    pub playhead: repose_core::signal::Signal<Timestamp>,
    pub project_path: repose_core::signal::Signal<Option<PathBuf>>,
}

pub struct Store {
    pub state: AppState,
    cmd_tx: Sender<BackendCommand>,
    clipboard: RefCell<Option<ClipboardContent>>,
    pub dock_state: Rc<RefCell<DockState>>,
    pub render_ctx: RefCell<Option<RenderContext>>,
    pub timeline_thumb_cache: Arc<Mutex<HashMap<(AssetId, i64), repose_core::ImageHandle>>>,
}

impl Clone for Store {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            cmd_tx: self.cmd_tx.clone(),
            clipboard: RefCell::new(self.clipboard.borrow().clone()),
            dock_state: self.dock_state.clone(),
            render_ctx: RefCell::new(self.render_ctx.borrow().clone()),
            timeline_thumb_cache: self.timeline_thumb_cache.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum BackendCommand {
    Project(ProjectCommand),
    Edit(EditCommand),
    Asset(AssetCommand),
    Playback(PlaybackCommand),
    Preview(PreviewCommand),
    Render(RenderCommand),
    Undo,
    Redo,
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
                timeline_snap: signal(true),
                last_render_plan_summary: signal(None),
                export_output_path: signal(None),
                export_quality: signal(QualityPreset::Standard),
                last_render_result: signal(None),
                preview_image_handle: signal(0),
                playhead: signal(Timestamp::ZERO),
                project_path: signal(None),
            },
            cmd_tx,
            clipboard: RefCell::new(None),
            dock_state: Rc::new(RefCell::new(dock_state)),
            render_ctx: RefCell::new(None),
            timeline_thumb_cache: Arc::new(Mutex::new(HashMap::new())),
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
    pub fn dispatch_edit(&self, cmd: EditCommand) {
        let _ = self.cmd_tx.send(BackendCommand::Edit(cmd));
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
    pub fn dispatch_preview(&self, cmd: PreviewCommand) {
        let _ = self.cmd_tx.send(BackendCommand::Preview(cmd));
    }
    pub fn dispatch_undo(&self) {
        let _ = self.cmd_tx.send(BackendCommand::Undo);
    }
    pub fn dispatch_redo(&self) {
        let _ = self.cmd_tx.send(BackendCommand::Redo);
    }

    pub fn copy_selected_clip(&self) {
        if let Some(clip_id) = self.state.selected_clip_id.get() {
            if let Some(timeline) = self.state.timeline.get() {
                let found = timeline.tracks.iter().find_map(|t| t.clip_by_id(clip_id));
                if let Some(clip) = found {
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

    pub fn cut_selected_clip(&self) {
        if let Some(clip_id) = self.state.selected_clip_id.get() {
            if let Some(timeline) = self.state.timeline.get() {
                let found = timeline.tracks.iter().find_map(|t| t.clip_by_id(clip_id));
                if let Some(clip) = found {
                    *self.clipboard.borrow_mut() = Some(ClipboardContent {
                        clip: clip.clone(),
                        is_cut: true,
                    });
                    self.dispatch_edit(EditCommand::RemoveClip { clip_id });
                    self.state.selected_clip_id.set(None);
                    self.state.status_msg.set("Clip cut".into());
                }
            }
        } else {
            self.state.status_msg.set("No clip selected".into());
        }
    }

    pub fn paste_clip(&self) {
        let clipboard = self.clipboard.borrow();
        if let Some(content) = clipboard.as_ref() {
            if let Some(timeline) = self.state.timeline.get() {
                let track_id = timeline.tracks.first().map(|t| t.id);
                if let Some(track_id) = track_id {
                    let playhead = self.state.playhead.get();
                    let mut clip = content.clip.clone();
                    clip.timeline_start = playhead;
                    // Use a new clip ID to avoid conflicts
                    clip.id = ClipId::new();
                    self.dispatch_edit(EditCommand::AddClip {
                        track_id,
                        clip,
                    });
                    self.state.status_msg.set("Clip pasted".into());
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
                self.state.project_path.set(None);
                self.state.playhead.set(Timestamp::ZERO);
                self.state.status_msg.set("Project initialized".into());
            }
            AppEvent::ProjectOpened { project } => {
                self.state.project.set(Some(project));
                self.state.playhead.set(Timestamp::ZERO);
                self.state.status_msg.set("Project opened".into());
            }
            AppEvent::ProjectSaved { path } => {
                self.state.project_path.set(Some(path));
                self.state.status_msg.set("Project saved".into());
            }
            AppEvent::ProjectClosed => {
                self.state.project.set(None);
                self.state.timeline.set(None);
                self.state.assets.set(vec![]);
                self.state.selected_asset_id.set(None);
                self.state.selected_clip_id.set(None);
                self.state.last_render_result.set(None);
                self.state.status_msg.set("Project closed".into());
            }

            AppEvent::TimelineUpdated { timeline } => {
                self.state.timeline.set(Some(timeline));
            }

            AppEvent::PlayheadMoved { timestamp } => {
                self.state.playhead.set(timestamp);
                self.state.status_msg.set(format!("Playhead: {}", timestamp.0));
                request_frame();
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

            AppEvent::PreviewFrameReady {
                timestamp: _,
                png_bytes,
            } => {
                if let Some(render_ctx) = self.render_ctx.borrow().clone() {
                    if let Ok(image) = image::load_from_memory(&png_bytes) {
                        let rgba = image.to_rgba8();
                        let (w, h) = rgba.dimensions();
                        render_ctx.set_image_rgba8(
                            self.state.preview_image_handle.get(),
                            w,
                            h,
                            rgba.into_raw(),
                            true,
                        );
                        request_frame();
                    }
                }
            }
            AppEvent::PreviewFrameFailed {
                timestamp: _,
                error,
            } => {
                self.state.status_msg.set(format!("Preview error: {error}"));
            }
            AppEvent::TimelineThumbnailReady {
                asset_id,
                source_time,
                png_bytes,
            } => {
                if let Some(render_ctx) = self.render_ctx.borrow().clone() {
                    if let Ok(image) = image::load_from_memory(&png_bytes) {
                        let rgba = image.to_rgba8();
                        let (w, h) = rgba.dimensions();
                        let handle = render_ctx.alloc_image_handle();
                        render_ctx.set_image_rgba8(handle, w, h, rgba.into_raw(), true);
                        if let Ok(mut cache) = self.timeline_thumb_cache.lock() {
                            cache.insert((asset_id, source_time), handle);
                        }
                        request_frame();
                    }
                }
            }
            AppEvent::TimelineThumbnailFailed { .. } => {}

            AppEvent::RenderPlanReady { plan } => {
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
