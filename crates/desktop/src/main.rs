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
        asset_service::AssetService, project_service::ProjectService,
        timeline_service::TimelineService,
    },
    AppEvent, AssetCommand, ProjectCommand, TimelineCommand,
};

mod state;
mod views;

use state::{BackendCommand, Store};

fn main() -> Result<()> {
    // Logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info,snapshort=debug"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // UI -> Backend (Commands)
    let (cmd_tx, cmd_rx) = unbounded::<BackendCommand>();

    // Backend -> UI (Events)
    let (evt_tx, evt_rx) = unbounded::<AppEvent>();

    // Spawn backend thread
    thread::spawn(move || run_backend(cmd_rx, evt_tx));

    // UI store (Rc, because UI is single-threaded)
    let store = Rc::new(Store::new(cmd_tx));

    // Run Repose App
    run_desktop_app(move |_sched| {
        // Poll events from backend
        while let Ok(event) = evt_rx.try_recv() {
            store.handle_event(event);
        }

        // Render
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
        // Determine DB path
        let proj_dirs = ProjectDirs::from("com", "mlm-games", "snapshort")
            .expect("Failed to resolve project directories");
        let data_dir = proj_dirs.data_dir();
        std::fs::create_dir_all(data_dir).ok();
        let db_path = data_dir.join("snapshort.db");

        // Proxy dir
        let proxy_dir = data_dir.join("proxies");
        std::fs::create_dir_all(&proxy_dir).ok();

        // Init DB
        let db = DbPool::new(&db_path).await.expect("DB init failed");

        // Init services + event bus
        let event_bus = snapshort_usecases::EventBus::new();
        let event_rx = event_bus.receiver();

        let project_service =
            std::sync::Arc::new(ProjectService::new(db.clone(), event_bus.clone()));
        let timeline_service =
            std::sync::Arc::new(TimelineService::new(db.clone(), event_bus.clone()));
        let asset_service =
            std::sync::Arc::new(AssetService::new(db.clone(), event_bus.clone(), proxy_dir));

        // Forwarder task: flume (async) -> crossbeam (sync UI)
        // Also performs required backend-side bookkeeping (set project, load timeline)
        {
            let tx = evt_tx.clone();
            let project_service = project_service.clone();
            let timeline_service = timeline_service.clone();
            let asset_service = asset_service.clone();

            tokio::spawn(async move {
                while let Ok(ev) = event_rx.recv_async().await {
                    // Backend bookkeeping (so app works without extra UI screens)
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

        // BOOTSTRAP: create an initial project immediately
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

        // Command loop
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
            }
        }
    });
}
