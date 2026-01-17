use flume::Sender;
use repose_core::signal::signal;
use snapshort_domain::prelude::*;
use snapshort_usecases::{
    AppEvent, AssetCommand, PlaybackCommand, ProjectCommand, TimelineCommand,
};

/// The single source of truth for the UI, using Repose signals
#[derive(Clone)]
pub struct AppState {
    pub project: repose_core::signal::Signal<Option<Project>>,
    pub assets: repose_core::signal::Signal<Vec<Asset>>,
    pub timeline: repose_core::signal::Signal<Option<Timeline>>,
    pub status_msg: repose_core::signal::Signal<String>,
    pub is_loading: repose_core::signal::Signal<bool>,

    // Selection
    pub selected_asset_id: repose_core::signal::Signal<Option<AssetId>>,
    pub selected_clip_id: repose_core::signal::Signal<Option<ClipId>>,
}

#[derive(Clone)]
pub struct Store {
    pub state: AppState,
    cmd_tx: Sender<BackendCommand>,
}

#[derive(Debug, Clone)]
pub enum BackendCommand {
    Project(ProjectCommand),
    Timeline(TimelineCommand),
    Asset(AssetCommand),
    Playback(PlaybackCommand),
}

impl Store {
    pub fn new(cmd_tx: Sender<BackendCommand>) -> Self {
        Self {
            state: AppState {
                project: signal(None),
                assets: signal(vec![]),
                timeline: signal(None),
                status_msg: signal("Ready".to_string()),
                is_loading: signal(false),
                selected_asset_id: signal(None),
                selected_clip_id: signal(None),
            },
            cmd_tx,
        }
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
                }
            }

            AppEvent::PlaybackStarted => self.state.status_msg.set("Playing".into()),
            AppEvent::PlaybackPaused => self.state.status_msg.set("Paused".into()),
            AppEvent::PlaybackStopped => self.state.status_msg.set("Stopped".into()),

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

            // Phase 1 jobs events: no UI yet (but keep match exhaustive)
            AppEvent::JobQueued { .. }
            | AppEvent::JobStarted { .. }
            | AppEvent::JobProgress { .. }
            | AppEvent::JobFinished { .. }
            | AppEvent::JobFailed { .. }
            | AppEvent::JobCanceled { .. } => {}

            AppEvent::UndoStackChanged { .. } => {}

            AppEvent::Error { message } => {
                self.state.status_msg.set(format!("Error: {}", message));
            }
        }
    }
}
