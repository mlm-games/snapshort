//! Snapshort Desktop Application with Repose UI

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Sender};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use repose_platform::run_desktop_app;
use snapshort_infra_db::DbPool;
use snapshort_usecases::{AppEvent, AssetService, EventBus, ProjectService, TimelineService};

mod state;
mod views;

use state::{BackendCommand, Store};

fn main() -> Result<()> {
    // 1. Setup Logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info,snapshort=debug"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 2. Setup Communication Channels
    // UI -> Backend (Commands)
    let (cmd_tx, cmd_rx) = unbounded::<BackendCommand>();
    // Backend -> UI (Events)
    let (evt_tx, evt_rx) = unbounded::<AppEvent>();

    // 3. Spawn Backend Thread (Tokio Runtime)
    thread::spawn(move || {
        run_backend(cmd_rx, evt_tx);
    });

    // 4. Initialize UI Store
    let store = std::rc::Rc::new(Store::new(cmd_tx));

    // 5. Run Repose App
    run_desktop_app(move |_sched| {
        // Poll events from backend
        while let Ok(event) = evt_rx.try_recv() {
            tracing::info!("UI Received event: {:?}", event);
            store.handle_event(event);
        }

        // Render Root View
        views::root_view(store.clone())
    });

    Ok(())
}

fn run_backend(cmd_rx: Receiver<BackendCommand>, evt_tx: Sender<AppEvent>) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("Failed to build tokio runtime");

    runtime.block_on(async {
        tracing::info!("Backend started");

        // Init Services
        let db_path = PathBuf::from("snapshort.db");
        let db = match DbPool::new(&db_path).await {
            Ok(db) => db,
            Err(e) => {
                tracing::error!("Failed to initialize DB: {}", e);
                return;
            }
        };

        // Custom EventBus that forwards to the crossbeam channel for UI
        let event_bus = EventBus::new();
        let event_rx = event_bus.receiver();

        // Forwarder task: Flume (Async) -> Crossbeam (Sync UI)
        let tx = evt_tx.clone();
        tokio::spawn(async move {
            while let Ok(ev) = event_rx.recv_async().await {
                if let Err(e) = tx.send(ev) {
                    tracing::warn!("Failed to forward event to UI: {}", e);
                    break;
                }
            }
        });

        let proxy_dir = PathBuf::from("proxies");
        std::fs::create_dir_all(&proxy_dir).ok();

        let project_service = Arc::new(ProjectService::new(db.clone(), event_bus.clone()));
        let timeline_service = Arc::new(TimelineService::new(db.clone(), event_bus.clone()));
        let asset_service = Arc::new(AssetService::new(db.clone(), event_bus.clone(), proxy_dir));

        tracing::info!("Backend ready");

        // Command Loop
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                BackendCommand::Project(c) => {
                    let s = project_service.clone();
                    let tx = evt_tx.clone();
                    tokio::spawn(async move {
                        // CRITICAL FIX: Handle result and report errors
                        if let Err(e) = s.execute(c).await {
                            tracing::error!("Project command failed: {}", e);
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
                            tracing::error!("Timeline command failed: {}", e);
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
                            tracing::error!("Asset command failed: {}", e);
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
