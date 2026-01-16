//! Snapshort Desktop Application with Repose UI

use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Sender};
use directories::ProjectDirs;
use std::path::PathBuf;
use std::rc::Rc;
use std::thread;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use repose_platform::run_desktop_app;
use snapshort_infra_db::DbPool;
use snapshort_usecases::{
    services::{
        asset_service::AssetService, playback_service::PlaybackService,
        project_service::ProjectService, timeline_service::TimelineService,
    },
    AppEvent, PlaybackCommand, ProjectCommand,
};

mod state;
mod views;

use state::{BackendCommand, Store};

fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info,snapshort=debug"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let (cmd_tx, cmd_rx) = unbounded::<BackendCommand>();
    let (evt_tx, evt_rx) = unbounded::<AppEvent>();

    thread::spawn(move || run_backend(cmd_rx, evt_tx));

    let store = Rc::new(Store::new(cmd_tx));

    run_desktop_app(move |_sched| {
        while let Ok(event) = evt_rx.try_recv() {
            store.handle_event(event);
        }
        views::root_view(store.clone())
    })?;

    Ok(())
}

fn run_backend(cmd_rx: Receiver<BackendCommand>, evt_tx: Sender<AppEvent>) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("Failed to build tokio runtime");

    runtime.block_on(async move {
        let proj_dirs = ProjectDirs::from("com", "mlm-games", "snapshort")
            .expect("Failed to resolve project directories");
        let data_dir = proj_dirs.data_dir();
        std::fs::create_dir_all(data_dir).ok();

        let db_path = data_dir.join("snapshort.db");
        let proxy_dir = data_dir.join("proxies");
        std::fs::create_dir_all(&proxy_dir).ok();

        let db = DbPool::new(&db_path).await.expect("DB init failed");

        let event_bus = snapshort_usecases::EventBus::new();
        let event_rx = event_bus.receiver();

        let project_service =
            std::sync::Arc::new(ProjectService::new(db.clone(), event_bus.clone()));
        let timeline_service =
            std::sync::Arc::new(TimelineService::new(db.clone(), event_bus.clone()));
        let asset_service =
            std::sync::Arc::new(AssetService::new(db.clone(), event_bus.clone(), proxy_dir));
        let playback_service = std::sync::Arc::new(PlaybackService::new(event_bus.clone()));

        // Forwarder: flume -> crossbeam UI
        {
            let tx = evt_tx.clone();
            let project_service = project_service.clone();
            let timeline_service = timeline_service.clone();
            let asset_service = asset_service.clone();

            tokio::spawn(async move {
                while let Ok(ev) = event_rx.recv_async().await {
                    match &ev {
                        AppEvent::ProjectCreated { project }
                        | AppEvent::ProjectOpened { project } => {
                            asset_service.set_project(project.id).await;
                            if let Some(tid) = project.active_timeline_id {
                                let _ = timeline_service.load(tid).await;
                            }
                        }
                        AppEvent::TimelineCreated { timeline } => {
                            let _ = timeline_service.load(timeline.id).await;
                        }
                        _ => {}
                    }
                    let _ = tx.send(ev);
                }
            });
        }

        // Bootstrap project immediately
        if let Err(e) = project_service
            .execute(ProjectCommand::Create {
                name: "Untitled".to_string(),
            })
            .await
        {
            let _ = evt_tx.send(AppEvent::Error {
                message: format!("Bootstrap project failed: {}", e),
            });
        }

        // Phase 3: set playback FPS default (optional)
        playback_service.set_fps(24).await;

        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                BackendCommand::Project(c) => {
                    let s = project_service.clone();
                    let tx = evt_tx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = s.execute(c).await {
                            let _ = tx.send(AppEvent::Error {
                                message: e.to_string(),
                            });
                        }
                    });
                }
                BackendCommand::Timeline(c) => {
                    let s = timeline_service.clone();
                    let tx = evt_tx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = s.execute(c).await {
                            let _ = tx.send(AppEvent::Error {
                                message: e.to_string(),
                            });
                        }
                    });
                }
                BackendCommand::Asset(c) => {
                    let s = asset_service.clone();
                    let tx = evt_tx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = s.execute(c).await {
                            let _ = tx.send(AppEvent::Error {
                                message: e.to_string(),
                            });
                        }
                    });
                }
                BackendCommand::Playback(c) => {
                    let s = playback_service.clone();
                    let tx = evt_tx.clone();
                    tokio::spawn(async move {
                        // PlaybackService doesn't return Result; keep this match consistent anyway
                        match c {
                            PlaybackCommand::Play => s.play().await,
                            PlaybackCommand::Pause => s.pause().await,
                            PlaybackCommand::Stop => s.stop().await,
                            PlaybackCommand::Seek { frame } => s.seek(frame.0).await,
                            PlaybackCommand::SetFps { fps } => s.set_fps(fps).await,
                        }

                        // no-op; but if you later make playback fallible, you already have tx here
                        let _ = tx; // keep unused warning away if you remove tx usage elsewhere
                    });
                }
            }
        }
    });
}
