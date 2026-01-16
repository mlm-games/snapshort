use crossbeam_channel::Sender;
use snapshort_domain::{Asset, Frame, Project, Timeline};
use snapshort_usecases::{
    AppEvent, AssetCommand, PlaybackCommand, ProjectCommand, TimelineCommand,
};
use std::rc::Rc;

use repose_core::signal::{signal, Signal};

/// The single source of truth for the UI, using Repose signals
#[derive(Clone)]
pub struct AppState {
    pub project: Signal<Option<Project>>,
    pub assets: Signal<Vec<Asset>>,
    pub timeline: Signal<Option<Timeline>>,

    pub status_msg: Signal<String>,
    pub is_loading: Signal<bool>,
}

#[derive(Clone)]
pub struct Store {
    pub state: Rc<AppState>,
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
        let state = AppState {
            project: signal(None),
            assets: signal(vec![]),
            timeline: signal(None),
            status_msg: signal("Ready".to_string()),
            is_loading: signal(false),
        };

        Self {
            state: Rc::new(state),
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
            AppEvent::ProjectCreated { project } | AppEvent::ProjectOpened { project } => {
                self.state.project.set(Some(project));
                self.state.status_msg.set("Project initialized".into());
            }
            AppEvent::ProjectClosed => {
                self.state.project.set(None);
                self.state.timeline.set(None);
                self.state.assets.set(vec![]);
                self.state.status_msg.set("Project closed".into());
            }

            AppEvent::TimelineCreated { timeline } | AppEvent::TimelineUpdated { timeline } => {
                self.state.timeline.set(Some(timeline));
            }

            // IMPORTANT: playback + seek update only sends frame; update timeline playhead locally
            AppEvent::PlayheadMoved { frame } => {
                if let Some(mut tl) = self.state.timeline.get() {
                    tl.playhead = frame;
                    self.state.timeline.set(Some(tl));
                }
            }

            AppEvent::PlaybackStarted => self.state.status_msg.set("Playing".into()),
            AppEvent::PlaybackPaused => self.state.status_msg.set("Paused".into()),
            AppEvent::PlaybackStopped => self.state.status_msg.set("Stopped".into()),

            AppEvent::AssetImported { asset } => {
                let mut list = self.state.assets.get();
                list.push(asset);
                self.state.assets.set(list);
            }
            AppEvent::AssetAnalyzed { asset }
            | AppEvent::AssetProxyComplete { asset }
            | AppEvent::AssetUpdated { asset } => {
                let mut list = self.state.assets.get();
                if let Some(i) = list.iter().position(|a| a.id == asset.id) {
                    list[i] = asset;
                } else {
                    list.push(asset);
                }
                self.state.assets.set(list);
            }
            AppEvent::AssetProxyProgress { asset_id, progress } => {
                let mut list = self.state.assets.get();
                if let Some(i) = list.iter().position(|a| a.id == asset_id) {
                    let mut a = list[i].clone();
                    a.status = snapshort_domain::AssetStatus::ProxyGenerating { progress };
                    list[i] = a;
                    self.state.assets.set(list);
                }
            }
            AppEvent::AssetDeleted { asset_id } => {
                let mut list = self.state.assets.get();
                list.retain(|a| a.id != asset_id);
                self.state.assets.set(list);
            }

            AppEvent::Error { message } => {
                self.state.status_msg.set(format!("Error: {}", message));
            }

            _ => {}
        }
    }
}
