use crate::views::timeline::menus::{MenuTarget, TopMenus};
use flume::Sender;
use miniter_domain::{Clip, ClipId, Timestamp, TrackId};
use miniter_usecases::EditCommand;
use repose_core::request_frame;
use repose_core::signal::signal;
use repose_docking::DockState;
use repose_platform::RenderContext;
use repose_ui::overlay::OverlayHandle;
use snapshort_infra_render::QualityPreset;
use snapshort_usecases::{
    AppEvent, Asset, AssetCommand, AssetId, AssetStatus, PlaybackCommand, PreviewCommand,
    ProjectCommand, RenderCommand,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct ClipboardContent {
    pub clip: Clip,
}

/// A clip waiting to be placed on an auto-created track. Set when the user
/// quick-adds or pastes a clip but no unlocked track of the matching kind
/// exists; flushed on the next `TimelineUpdated` once the track appears.
#[derive(Debug, Clone)]
pub struct PendingClipAdd {
    pub kind: miniter_domain::TrackKind,
    pub clip: Clip,
}

#[derive(Clone)]
pub struct AppState {
    pub project: repose_core::signal::Signal<Option<miniter_domain::Project>>,
    pub assets: repose_core::signal::Signal<Vec<Asset>>,
    pub timeline: repose_core::signal::Signal<Option<miniter_domain::Timeline>>,
    pub status_msg: repose_core::signal::Signal<String>,
    /// A genuinely blocking operation is in progress (full-screen overlay).
    pub blocking_operation: repose_core::signal::Signal<Option<String>>,
    /// Number of background jobs (analyze / proxy) running; shown in the
    /// status bar instead of blocking the editor.
    pub background_jobs: repose_core::signal::Signal<u32>,
    pub can_undo: repose_core::signal::Signal<bool>,
    pub can_redo: repose_core::signal::Signal<bool>,
    pub playback_state: repose_core::signal::Signal<String>,
    pub last_error: repose_core::signal::Signal<Option<String>>,
    pub selected_asset_id: repose_core::signal::Signal<Option<AssetId>>,
    pub selected_clip_id: repose_core::signal::Signal<Option<ClipId>>,
    pub timeline_zoom: repose_core::signal::Signal<f32>,
    pub timeline_snap: repose_core::signal::Signal<bool>,
    pub timeline_snap_indicator: repose_core::signal::Signal<Option<Timestamp>>,
    pub last_render_plan_summary: repose_core::signal::Signal<Option<String>>,
    pub export_output_path: repose_core::signal::Signal<Option<PathBuf>>,
    pub export_quality: repose_core::signal::Signal<QualityPreset>,
    pub last_render_result: repose_core::signal::Signal<Option<String>>,
    pub preview_image_handle: repose_core::signal::Signal<repose_core::ImageHandle>,
    /// Last playhead position we requested a preview frame for, so the monitor
    /// doesn't re-request the same frame every render.
    pub last_requested_preview_us: repose_core::signal::Signal<Option<i64>>,
    pub playhead: repose_core::signal::Signal<Timestamp>,
    pub project_path: repose_core::signal::Signal<Option<PathBuf>>,
    pub drag_hover_track: repose_core::signal::Signal<Option<TrackId>>,
    pub project_dirty: repose_core::signal::Signal<bool>,
    pub asset_search_query: repose_core::signal::Signal<String>,
    pub timeline_markers: repose_core::signal::Signal<Vec<TimelineMarker>>,
    pub track_volumes: repose_core::signal::Signal<HashMap<TrackId, f32>>,
    pub track_solos: repose_core::signal::Signal<HashSet<TrackId>>,
    pub master_volume: repose_core::signal::Signal<f32>,
    pub panel_origin: std::rc::Rc<std::cell::RefCell<Option<repose_core::Vec2>>>,
    pub clip_menu: MenuTarget,
    pub clip_menu_target: repose_core::signal::Signal<Option<(ClipId, TrackId)>>,
    pub track_menu: MenuTarget,
    pub track_menu_target: repose_core::signal::Signal<Option<TrackId>>,
    pub add_track_menu: MenuTarget,
    pub top_menus: TopMenus,
    pub pending_clip_add: repose_core::signal::Signal<Option<PendingClipAdd>>,
    /// Inspector expand/collapse and transient string state, keyed by
    /// (clip-id, param) so it survives recomposition without leaking through
    /// process-wide thread-locals.
    pub inspector_flags: std::rc::Rc<std::cell::RefCell<HashMap<String, bool>>>,
    pub inspector_strings: std::rc::Rc<std::cell::RefCell<HashMap<String, String>>>,
}

#[derive(Debug, Clone)]
pub struct TimelineMarker {
    pub timestamp_us: i64,
    pub label: String,
}

pub struct Store {
    pub state: AppState,
    cmd_tx: Sender<BackendCommand>,
    clipboard: RefCell<Option<ClipboardContent>>,
    pub dock_state: Rc<RefCell<DockState>>,
    pub render_ctx: RefCell<Option<RenderContext>>,
    pub overlay: OverlayHandle,
    pub timeline_thumb_cache: Arc<Mutex<HashMap<(AssetId, i64), repose_core::ImageHandle>>>,
    pub timeline_thumb_pending: Arc<Mutex<HashSet<(AssetId, i64)>>>,
}

impl Clone for Store {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            cmd_tx: self.cmd_tx.clone(),
            clipboard: RefCell::new(self.clipboard.borrow().clone()),
            dock_state: self.dock_state.clone(),
            render_ctx: RefCell::new(self.render_ctx.borrow().clone()),
            overlay: self.overlay.clone(),
            timeline_thumb_cache: self.timeline_thumb_cache.clone(),
            timeline_thumb_pending: self.timeline_thumb_pending.clone(),
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
                blocking_operation: signal(None),
                background_jobs: signal(0),
                can_undo: signal(false),
                can_redo: signal(false),
                playback_state: signal("Stopped".to_string()),
                last_error: signal(None),
                selected_asset_id: signal(None),
                selected_clip_id: signal(None),
                timeline_zoom: signal(2.0),
                timeline_snap: signal(true),
                timeline_snap_indicator: signal(None),
                last_render_plan_summary: signal(None),
                export_output_path: signal(None),
                export_quality: signal(QualityPreset::Standard),
                last_render_result: signal(None),
                preview_image_handle: signal(0),
                last_requested_preview_us: signal(None),
                playhead: signal(Timestamp::ZERO),
                project_path: signal(None),
                drag_hover_track: signal(None),
                project_dirty: signal(false),
                asset_search_query: signal(String::new()),
                timeline_markers: signal(Vec::new()),
                track_volumes: signal(HashMap::new()),
                track_solos: signal(HashSet::new()),
                master_volume: signal(1.0),
                panel_origin: Rc::new(RefCell::new(None)),
                clip_menu: MenuTarget::new(),
                clip_menu_target: signal(None),
                track_menu: MenuTarget::new(),
                track_menu_target: signal(None),
                add_track_menu: MenuTarget::new(),
                top_menus: TopMenus::new(),
                pending_clip_add: signal(None),
                inspector_flags: Rc::new(RefCell::new(HashMap::new())),
                inspector_strings: Rc::new(RefCell::new(HashMap::new())),
            },
            cmd_tx,
            clipboard: RefCell::new(None),
            dock_state: Rc::new(RefCell::new(dock_state)),
            render_ctx: RefCell::new(None),
            overlay: OverlayHandle::new(),
            timeline_thumb_cache: Arc::new(Mutex::new(HashMap::new())),
            timeline_thumb_pending: Arc::new(Mutex::new(HashSet::new())),
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

    pub fn open_clip_menu(
        &self,
        window_pos: repose_core::Vec2,
        clip_id: ClipId,
        track_id: TrackId,
    ) {
        if let Some(origin) = *self.state.panel_origin.borrow() {
            self.state.clip_menu.open_at_window(window_pos, origin);
            self.state.clip_menu_target.set(Some((clip_id, track_id)));
        }
    }

    pub fn open_track_menu(&self, window_pos: repose_core::Vec2, track_id: TrackId) {
        if let Some(origin) = *self.state.panel_origin.borrow() {
            self.state.track_menu.open_at_window(window_pos, origin);
            self.state.track_menu_target.set(Some(track_id));
        }
    }

    pub fn open_add_track_menu(&self, window_pos: repose_core::Vec2) {
        if let Some(origin) = *self.state.panel_origin.borrow() {
            self.state.add_track_menu.open_at_window(window_pos, origin);
        }
    }

    pub fn copy_selected_clip(&self) {
        if let Some(clip_id) = self.state.selected_clip_id.get() {
            if let Some(timeline) = self.state.timeline.get() {
                let found = timeline.tracks.iter().find_map(|t| t.clip_by_id(clip_id));
                if let Some(clip) = found {
                    *self.clipboard.borrow_mut() = Some(ClipboardContent { clip: clip.clone() });
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
                    *self.clipboard.borrow_mut() = Some(ClipboardContent { clip: clip.clone() });
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
            let playhead = self.state.playhead.get();
            let mut clip = content.clip.clone();
            clip.timeline_start = playhead;
            // Use a new clip ID to avoid conflicts.
            clip.id = ClipId::new();
            self.add_clip_to_preferred_track(clip);
            self.state.status_msg.set("Clip pasted".into());
        } else {
            self.state.status_msg.set("Clipboard is empty".into());
        }
    }

    pub fn has_clipboard(&self) -> bool {
        self.clipboard.borrow().is_some()
    }

    /// Place a clip on the preferred track for its kind (first unlocked track
    /// of the matching kind). When no such track exists, auto-create one and
    /// place the clip as soon as it appears (`pending_clip_add`).
    pub fn add_clip_to_preferred_track(&self, clip: Clip) {
        let Some(timeline) = self.state.timeline.get() else {
            return;
        };
        let kind = crate::views::timeline::track_kind_for_clip(&clip);
        if let Some(track_id) =
            crate::views::timeline::preferred_track_for_kind(&timeline, kind)
        {
            self.dispatch_edit(EditCommand::AddClip { track_id, clip });
            return;
        }
        self.state.pending_clip_add.set(Some(PendingClipAdd {
            kind,
            clip,
        }));
        let name = crate::views::timeline::next_track_name(&timeline, kind);
        self.dispatch_edit(EditCommand::AddTrack { kind, name });
    }

    pub fn handle_event(&self, event: AppEvent) {
        match event {
            AppEvent::ProjectCreated { project } => {
                self.state.project.set(Some(project));
                self.state.project_path.set(None);
                self.state.playhead.set(Timestamp::ZERO);
                self.state.project_dirty.set(false);
                self.state.pending_clip_add.set(None);
                self.state.selected_clip_id.set(None);
                self.state.selected_asset_id.set(None);
                self.state.status_msg.set("Project initialized".into());
            }
            AppEvent::ProjectOpened {
                project,
                timeline_markers,
            } => {
                self.state.project.set(Some(project));
                self.state.playhead.set(Timestamp::ZERO);
                self.state.project_dirty.set(false);
                self.state.timeline_markers.set(
                    timeline_markers
                        .into_iter()
                        .map(|m| TimelineMarker {
                            timestamp_us: m.timestamp_us,
                            label: m.label,
                        })
                        .collect(),
                );
                self.state.status_msg.set("Project opened".into());
            }
            AppEvent::ProjectSaved { path } => {
                self.state.project_path.set(Some(path));
                self.state.project_dirty.set(false);
                self.state.status_msg.set("Project saved".into());
            }
            AppEvent::ProjectClosed => {
                self.state.project.set(None);
                self.state.timeline.set(None);
                self.state.assets.set(vec![]);
                self.state.selected_asset_id.set(None);
                self.state.selected_clip_id.set(None);
                self.state.last_render_result.set(None);
                self.state.project_dirty.set(false);
                self.state.asset_search_query.set(String::new());
                self.state.timeline_markers.set(Vec::new());
                self.state.pending_clip_add.set(None);
                self.state.blocking_operation.set(None);
                self.state.background_jobs.set(0);
                self.state.can_undo.set(false);
                self.state.can_redo.set(false);
                self.state.last_requested_preview_us.set(None);
                self.state.drag_hover_track.set(None);
                self.state.timeline_snap_indicator.set(None);
                self.state.playhead.set(Timestamp::ZERO);
                self.state.status_msg.set("Project closed".into());
                if let Ok(mut cache) = self.timeline_thumb_cache.lock() {
                    cache.clear();
                }
                if let Ok(mut pending) = self.timeline_thumb_pending.lock() {
                    pending.clear();
                }
            }

            AppEvent::TimelineUpdated { timeline } => {
                // If a clip is queued for an auto-created track, place it now
                // that the track exists.
                if let Some(pending) = self.state.pending_clip_add.get() {
                    if let Some(track_id) =
                        crate::views::timeline::preferred_track_for_kind(&timeline, pending.kind)
                    {
                        self.state.pending_clip_add.set(None);
                        self.dispatch_edit(EditCommand::AddClip {
                            track_id,
                            clip: pending.clip,
                        });
                    }
                }
                self.state.timeline.set(Some(timeline));
                self.state.project_dirty.set(true);
            }

            AppEvent::PlayheadMoved { timestamp } => {
                self.state.playhead.set(timestamp);
                self.state
                    .status_msg
                    .set(format!("Playhead: {}", timestamp.0));
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
                        if let Ok(mut pending) = self.timeline_thumb_pending.lock() {
                            pending.remove(&(asset_id, source_time));
                        }
                        request_frame();
                    }
                }
            }
            AppEvent::TimelineThumbnailFailed {
                asset_id,
                source_time,
                ..
            } => {
                if let Ok(mut pending) = self.timeline_thumb_pending.lock() {
                    pending.remove(&(asset_id, source_time));
                }
            }

            AppEvent::RenderPlanReady { plan } => {
                self.state.last_render_plan_summary.set(Some(format!(
                    "Render plan ready: {} clips",
                    plan.clips.len()
                )));
                self.state.status_msg.set("Render plan ready".into());
            }
            AppEvent::RenderStarted { settings } => {
                self.state.blocking_operation.set(Some(format!(
                    "Exporting to {}…",
                    settings.output_path.display()
                )));
                self.state
                    .status_msg
                    .set(format!("Exporting to {}…", settings.output_path.display()));
                self.state.last_render_result.set(None);
            }
            AppEvent::RenderFinished { result } => {
                self.state.blocking_operation.set(None);
                self.state.status_msg.set("Export complete".into());
                self.state.last_render_result.set(Some(format!(
                    "Exported to {}",
                    result.output_path.display()
                )));
            }
            AppEvent::RenderFailed { error } => {
                self.state.blocking_operation.set(None);
                self.state.status_msg.set("Export failed".into());
                self.state
                    .last_render_result
                    .set(Some(format!("Export failed: {error}")));
            }

            AppEvent::AssetsLoaded { assets } => {
                self.state.assets.set(assets);
                self.state.asset_search_query.set(String::new());
            }

            AppEvent::AssetImported { asset } => {
                let mut list = self.state.assets.get();
                list.push(asset);
                self.state.assets.set(list);
                self.state.project_dirty.set(true);
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
                self.state.project_dirty.set(true);

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

            // Analyze / proxy jobs are background work: bump a counter shown in
            // the status bar rather than blocking the editor with an overlay.
            AppEvent::JobQueued { kind, .. } => {
                // Only genuinely blocking operations warrant the overlay;
                // analyze/proxy (the current jobs) are handled by the
                // started/finished handlers below.
                self.state.status_msg.set(format!("Queued: {kind}"));
            }
            AppEvent::JobStarted { .. } => {
                self.state.background_jobs.set(self.state.background_jobs.get() + 1);
            }
            AppEvent::JobProgress { progress, .. } => {
                self.state.status_msg.set(format!("Background job… {progress}%"));
            }
            AppEvent::JobFinished { .. }
            | AppEvent::JobFailed { .. }
            | AppEvent::JobCanceled { .. } => {
                let n = self.state.background_jobs.get().saturating_sub(1);
                self.state.background_jobs.set(n);
                if n == 0 {
                    self.state.status_msg.set("Ready".into());
                }
            }

            AppEvent::UndoStackChanged { can_undo, can_redo } => {
                self.state.can_undo.set(can_undo);
                self.state.can_redo.set(can_redo);
            }

            AppEvent::Error { message } => {
                self.state.last_error.set(Some(message.clone()));
                self.state.status_msg.set(format!("Error: {}", message));
            }
        }
    }
}
