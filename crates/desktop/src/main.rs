//! Snapshort Desktop Application with Repose UI
use anyhow::Result;
use directories::ProjectDirs;
use flume::{Receiver, Sender};
use repose_platform::run_desktop_app;
use snapshort_domain::{Timeline, TimelineSettings};
use snapshort_infra_db::{DbPool, TimelineRepository};
use snapshort_usecases::{
    AppEvent, AssetService, EventBus, JobsService, PlaybackCommand, PlaybackService,
    ProjectCommand, ProjectService, TimelineCommand, TimelineService,
};
use std::path::PathBuf;
use std::rc::Rc;
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
    let (evt_tx, evt_rx) = flume::unbounded::<AppEvent>();

    // Create default dock layout
    let dock_state = views::panels::create_default_layout();
    let store = Rc::new(Store::new(cmd_tx, dock_state));

    thread::spawn(move || run_backend(cmd_rx, evt_tx));

    run_desktop_app(move |_sched, _ctx| {
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
        let event_bus = EventBus::new();
        let event_rx = event_bus.receiver();

        // Services
        let jobs = std::sync::Arc::new(JobsService::new(db.clone(), event_bus.clone(), proxy_dir));
        jobs.recover_and_resume().await.ok();

        let project_service =
            std::sync::Arc::new(ProjectService::new(db.clone(), event_bus.clone()));
        let timeline_service =
            std::sync::Arc::new(TimelineService::new(db.clone(), event_bus.clone()));
        let asset_service = std::sync::Arc::new(AssetService::new(
            db.clone(),
            event_bus.clone(),
            jobs.clone(),
        ));
        let playback_service = std::sync::Arc::new(PlaybackService::new(event_bus.clone()));
        playback_service.set_fps(24).await;

        // Forwarder: event bus -> UI flume + orchestration hooks
        tokio::spawn({
            let tx = evt_tx.clone();
            let asset_service = asset_service.clone();
            let timeline_service = timeline_service.clone();
            let playback_service = playback_service.clone();

            async move {
                while let Ok(ev) = event_rx.recv_async().await {
                    // On project created/opened:
                    // - set asset service project id
                    // - bulk load assets
                    // - load active timeline
                    if let AppEvent::ProjectCreated { project }
                    | AppEvent::ProjectOpened { project } = &ev
                    {
                        asset_service.set_project(project.id).await;

                        if let Ok(assets) = asset_service.list().await {
                            let _ = tx.send(AppEvent::AssetsLoaded { assets });
                        }

                        if let Some(tid) = project.active_timeline_id {
                            tracing::info!("Attempting to load active timeline: {}", tid.0);
                            match timeline_service.load(tid).await {
                                Ok(timeline) => {
                                    tracing::info!("Successfully loaded timeline: {}", timeline.name);
                                }
                                Err(e) => {
                                    tracing::error!("Failed to load timeline {}: {}, creating fallback timeline", tid.0, e);
                                    let _ = tx.send(AppEvent::Error {
                                        message: format!("Failed to load timeline, creating new one: {}", e),
                                    });

                                    let new_timeline = Timeline::new("Timeline 1").with_settings(TimelineSettings {
                                        fps: project.settings.fps,
                                        resolution: project.settings.resolution,
                                        sample_rate: project.settings.sample_rate,
                                        audio_channels: 2,
                                    });

                                    match timeline_service.timeline_repo.create(project.id, &new_timeline).await {
                                        Ok(_) => {
                                            tracing::info!("Created fallback timeline: {}", new_timeline.name);
                                            if let Err(load_err) = timeline_service.load(new_timeline.id).await {
                                                tracing::error!("Failed to load newly created timeline {}: {}", new_timeline.id.0, load_err);
                                            } else {
                                                let _ = tx.send(AppEvent::TimelineCreated { timeline: new_timeline });
                                            }
                                        }
                                        Err(create_err) => {
                                            tracing::error!("Failed to create fallback timeline: {}", create_err);
                                            let _ = tx.send(AppEvent::Error {
                                                message: format!("Cannot create timeline: {}", create_err),
                                            });
                                        }
                                    }
                                }
                            }
                        } else {
                            tracing::warn!("No active timeline set for project, creating default timeline");
                            let new_timeline = Timeline::new("Timeline 1").with_settings(TimelineSettings {
                                fps: project.settings.fps,
                                resolution: project.settings.resolution,
                                sample_rate: project.settings.sample_rate,
                                audio_channels: 2,
                            });

                            match timeline_service.timeline_repo.create(project.id, &new_timeline).await {
                                Ok(_) => {
                                    tracing::info!("Created default timeline: {}", new_timeline.name);
                                    if let Err(load_err) = timeline_service.load(new_timeline.id).await {
                                        tracing::error!("Failed to load newly created timeline {}: {}", new_timeline.id.0, load_err);
                                    } else {
                                        let _ = tx.send(AppEvent::TimelineCreated { timeline: new_timeline });
                                    }
                                }
                                Err(create_err) => {
                                    tracing::error!("Failed to create default timeline: {}", create_err);
                                    let _ = tx.send(AppEvent::Error {
                                        message: format!("Cannot create timeline: {}", create_err),
                                    });
                                }
                            }
                        }
                    }

                    // Keep playback bounded by current timeline end
                    if let AppEvent::TimelineUpdated { timeline }
                    | AppEvent::TimelineCreated { timeline } = &ev
                    {
                        playback_service
                            .set_max_frame(Some(timeline.duration().0))
                            .await;
                    }

                    let _ = tx.send(ev);
                }
            }
        });

        // Startup: open most recent project if exists; otherwise create one.
        match project_service.list_projects().await {
            Ok(projects) if !projects.is_empty() => {
                tracing::info!(
                    "Found {} existing projects, opening most recent",
                    projects.len()
                );
                let p = &projects[0];
                tracing::info!("Opening project: {} ({})", p.name, p.id.0);
                let _ = project_service
                    .execute(ProjectCommand::Open {
                        path: PathBuf::from(p.id.0.to_string()),
                    })
                    .await;
            }
            _ => {
                tracing::info!("No existing projects found, creating new project");
                if let Err(e) = project_service
                    .execute(ProjectCommand::Create {
                        name: "Untitled".to_string(),
                    })
                    .await
                {
                    tracing::error!("Bootstrap project failed: {}", e);
                    let _ = evt_tx.send(AppEvent::Error {
                        message: format!("Bootstrap project failed: {}", e),
                    });
                }
            }
        }

        // Main command loop
        while let Ok(cmd) = cmd_rx.recv_async().await {
            match cmd {
                BackendCommand::Project(c) => {
                    if let Err(e) = project_service.execute(c).await {
                        let _ = evt_tx.send(AppEvent::Error {
                            message: e.to_string(),
                        });
                    }
                }

                BackendCommand::Timeline(c) => {
                    let should_save = !matches!(c, TimelineCommand::Seek { .. });
                    if let Err(e) = timeline_service.execute(c).await {
                        let _ = evt_tx.send(AppEvent::Error {
                            message: e.to_string(),
                        });
                    } else if should_save {
                        let _ = timeline_service.save().await; // best effort auto-save
                    }
                }

                BackendCommand::Asset(c) => {
                    if let Err(e) = asset_service.execute(c).await {
                        let _ = evt_tx.send(AppEvent::Error {
                            message: e.to_string(),
                        });
                    }
                }

                BackendCommand::Playback(c) => match c {
                    PlaybackCommand::Play => playback_service.play().await,
                    PlaybackCommand::Pause => playback_service.pause().await,
                    PlaybackCommand::Stop => playback_service.stop().await,
                    PlaybackCommand::Seek { frame } => playback_service.seek(frame.0).await,
                    PlaybackCommand::SetFps { fps } => playback_service.set_fps(fps).await,
                },
            }
        }
    });
}
