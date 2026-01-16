use std::rc::Rc;
use repose_core::signal::{signal, Signal};
use snapshort_domain::prelude::*;
use snapshort_usecases::{AppEvent, ProjectCommand, TimelineCommand, AssetCommand};
use crossbeam_channel::Sender;

/// The single source of truth for the UI, using Repose signals
#[derive(Clone)]
pub struct AppState {
    // Data Signals
    pub project: Signal<Option<Project>>,
    pub assets: Signal<Vec<Asset>>,
    pub timeline: Signal<Option<Timeline>>,

    // UI State Signals
    pub current_view: Signal<ViewMode>,
    pub status_msg: Signal<String>,
    pub is_loading: Signal<bool>,
}

#[derive(Clone, Copy, PartialEq, Default)]
pub enum ViewMode {
    #[default]
    Editor,
    Welcome,
}

/// Action dispatcher that sends commands to the async backend
#[derive(Clone)]
pub struct Store {
    pub state: AppState,
    cmd_tx: Sender<BackendCommand>,
}

pub enum BackendCommand {
    Project(ProjectCommand),
    Timeline(TimelineCommand),
    Asset(AssetCommand),
}

impl Store {
    pub fn new(cmd_tx: Sender<BackendCommand>) -> Self {
        Self {
            state: AppState {
                project: signal(None),
                assets: signal(vec![]),
                timeline: signal(None),
                current_view: signal(ViewMode::Welcome),
                status_msg: signal("Ready".to_string()),
                is_loading: signal(false),
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

    /// Process events received from the backend (on the UI thread)
    pub fn handle_event(&self, event: AppEvent) {
        match event {
            AppEvent::ProjectCreated { project } | AppEvent::ProjectOpened { project } => {
                self.state.project.set(Some(project));
                self.state.current_view.set(ViewMode::Editor);
                self.state.status_msg.set("Project loaded".into());
            }
            AppEvent::ProjectClosed => {
                self.state.project.set(None);
                self.state.current_view.set(ViewMode::Welcome);
            }
            AppEvent::AssetImported { asset } => {
                let mut list = self.state.assets.get();
                list.push(asset);
                self.state.assets.set(list);
            }
            AppEvent::TimelineCreated { timeline } | AppEvent::TimelineUpdated { timeline } => {
                self.state.timeline.set(Some(timeline));
            }
            AppEvent::Error { message } => {
                self.state.status_msg.set(format!("Error: {}", message));
            }
            _ => {}
        }
    }
}
