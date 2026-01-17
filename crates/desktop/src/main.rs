//! Snapshort Desktop Application with Repose UI
use anyhow::Result;
use directories::ProjectDirs;
use flume::{Receiver, Sender};
use repose_platform::run_desktop_app;
use snapshort_infra_db::DbPool;
use snapshort_usecases::{
    AssetService, EventBus, JobsService, PlaybackService, ProjectCommand, ProjectService,
    TimelineService,
};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::thread;
use tracing_subscriber::prelude::*;

mod state;
mod views;

use state::Store;

use crate::state::BackendCommand;

fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info,snapshort=debug"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let (cmd_tx, cmd_rx) = flume::unbounded::<BackendCommand>();
    let (evt_tx, evt_rx) = flume::unbounded::<snapshort_usecases::AppEvent>();

    let store = Rc::new(Store::new(cmd_tx));

    thread::spawn(move || run_backend(cmd_rx, evt_tx));

    run_desktop_app(move |_sched| {
        while let Ok(event) = evt_rx.try_recv() {
            store.handle_event(event);
        }
        views::root_view(store.clone())
    })?;

    Ok(())
}

fn run_backend(cmd_rx: Receiver<BackendCommand>, evt_tx: Sender<snapshort_usecases::AppEvent>) {
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

        let event_bus = EventBus::new();
        let event_rx = event_bus.receiver();

        // Phase 1: Jobs first
        let jobs = Arc::new(JobsService::new(db.clone(), event_bus.clone(), proxy_dir));
        jobs.recover_and_resume().await.ok();

        let project_service = Arc::new(ProjectService::new(db.clone(), event_bus.clone()));
        let timeline_service = Arc::new(TimelineService::new(db.clone(), event_bus.clone()));
        let asset_service = Arc::new(AssetService::new(
            db.clone(),
            event_bus.clone(),
            jobs.clone(),
        ));
        let playback_service = Arc::new(PlaybackService::new(event_bus.clone()));

        // Forwarder: flume -> UI flume + do small orchestration hooks
        tokio::spawn({
            let tx = evt_tx.clone();
            let asset_service = asset_service.clone();
            let timeline_service = timeline_service.clone();
            async move {
                while let Ok(ev) = event_rx.recv_async().await {
                    // Hook: when a project is created/opened, ensure assets svc knows the project id,
                    // and load active timeline.
                    if let snapshort_usecases::AppEvent::ProjectCreated { project }
                    | snapshort_usecases::AppEvent::ProjectOpened { project } = &ev
                    {
                        asset_service.set_project(project.id).await;
                        if let Some(tid) = project.active_timeline_id {
                            let _ = timeline_service.load(tid).await;
                        }
                    }

                    let _ = tx.send(ev);
                }
            }
        });

        // Bootstrap project immediately (existing behavior)
        if let Err(e) = project_service
            .execute(ProjectCommand::Create {
                name: "Untitled".to_string(),
            })
            .await
        {
            let _ = evt_tx.send(snapshort_usecases::AppEvent::Error {
                message: format!("Bootstrap project failed: {}", e),
            });
        }

        playback_service.set_fps(24).await;

        // Main command loop (async-safe)
        while let Ok(cmd) = cmd_rx.recv_async().await {
            match cmd {
                BackendCommand::Project(c) => {
                    if let Err(e) = project_service.execute(c).await {
                        let _ = evt_tx.send(snapshort_usecases::AppEvent::Error {
                            message: e.to_string(),
                        });
                    }
                }
                BackendCommand::Timeline(c) => {
                    if let Err(e) = timeline_service.execute(c).await {
                        let _ = evt_tx.send(snapshort_usecases::AppEvent::Error {
                            message: e.to_string(),
                        });
                    }
                }
                BackendCommand::Asset(c) => {
                    if let Err(e) = asset_service.execute(c).await {
                        let _ = evt_tx.send(snapshort_usecases::AppEvent::Error {
                            message: e.to_string(),
                        });
                    }
                }
                BackendCommand::Playback(c) => match c {
                    snapshort_usecases::PlaybackCommand::Play => playback_service.play().await,
                    snapshort_usecases::PlaybackCommand::Pause => playback_service.pause().await,
                    snapshort_usecases::PlaybackCommand::Stop => playback_service.stop().await,
                    snapshort_usecases::PlaybackCommand::Seek { frame } => {
                        playback_service.seek(frame.0).await
                    }
                    snapshort_usecases::PlaybackCommand::SetFps { fps } => {
                        playback_service.set_fps(fps).await
                    }
                },
            }
        }
    });
}
