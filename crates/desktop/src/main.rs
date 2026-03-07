//! Snapshort Desktop Application with Repose UI
use anyhow::Result;
use directories::ProjectDirs;
use flume::{Receiver, Sender};
use repose_core::request_frame;
use repose_platform::run_desktop_app;
use snapshort_domain::{Timeline, TimelineSettings};
use snapshort_infra_db::{DbPool, TimelineRepository};
use snapshort_usecases::{
    AppEvent, AssetService, EventBus, JobsService, PlaybackCommand, PlaybackService,
    PreviewCommand, PreviewService, ProjectCommand, ProjectService, RenderCommand,
    TimelineCommand, TimelineService,
};
use std::rc::Rc;
use std::thread;
use tracing_subscriber::prelude::*;

mod state;
mod views;

use state::Store;

use crate::state::BackendCommand;

const DEFAULT_PROJECT_FILE_NAME: &str = "project.snap";

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

    run_desktop_app(move |_sched, ctx| {
        store.ensure_render_context(ctx);
        while let Ok(event) = evt_rx.try_recv() {
            store.handle_event(event);
        }
        views::root_view(store.clone())
    })?;

    Ok(())
}

fn send_ui_event(tx: &Sender<AppEvent>, event: AppEvent) {
    let _ = tx.send(event);
    request_frame();
}

fn run_backend(cmd_rx: Receiver<BackendCommand>, evt_tx: Sender<AppEvent>) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build();

    let Ok(runtime) = runtime else {
        let _ = evt_tx.send(AppEvent::Error {
            message: "Failed to build async runtime".into(),
        });
        return;
    };

    runtime.block_on(async move {
        let Some(proj_dirs) = ProjectDirs::from("com", "mlm-games", "snapshort") else {
            send_ui_event(
                &evt_tx,
                AppEvent::Error {
                    message: "Failed to resolve project directories".into(),
                },
            );
            return;
        };
        let data_dir = proj_dirs.data_dir();
        std::fs::create_dir_all(data_dir).ok();

        let db_path = data_dir.join("snapshort.db");
        let proxy_dir = data_dir.join("proxies");
        std::fs::create_dir_all(&proxy_dir).ok();

        let db = match DbPool::new(&db_path).await {
            Ok(db) => db,
            Err(e) => {
                send_ui_event(
                    &evt_tx,
                    AppEvent::Error {
                        message: format!("DB init failed: {e}"),
                    },
                );
                return;
            }
        };
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

        let render_service = std::sync::Arc::new(snapshort_infra_render::RenderService::new());
        let preview_service = std::sync::Arc::new(PreviewService::new(
            event_bus.clone(),
            render_service.clone(),
        ));

        // Forwarder: event bus -> UI flume + orchestration hooks
        tokio::spawn({
            let tx = evt_tx.clone();
            let asset_service = asset_service.clone();
            let timeline_service = timeline_service.clone();
            let playback_service = playback_service.clone();
            let project_service = project_service.clone();
            let preview_service = preview_service.clone();

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
                            preview_service.update_assets(assets.clone()).await;
                            send_ui_event(&tx, AppEvent::AssetsLoaded { assets });
                        }

                        if let Some(tid) = project.active_timeline_id {
                            tracing::info!("Attempting to load active timeline: {}", tid.0);
                            match timeline_service.load(tid).await {
                                Ok(timeline) => {
                                    tracing::info!("Successfully loaded timeline: {}", timeline.name);
                                }
                                Err(e) => {
                                    tracing::error!("Failed to load timeline {}: {}, creating fallback timeline", tid.0, e);
                                    send_ui_event(
                                        &tx,
                                        AppEvent::Error {
                                            message: format!(
                                                "Failed to load timeline, creating new one: {}",
                                                e
                                            ),
                                        },
                                    );

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
                                            send_ui_event(
                                                &tx,
                                                AppEvent::TimelineCreated {
                                                    timeline: new_timeline,
                                                },
                                            );
                                            }
                                        }
                                        Err(create_err) => {
                                            tracing::error!("Failed to create fallback timeline: {}", create_err);
                                            send_ui_event(
                                                &tx,
                                                AppEvent::Error {
                                                    message: format!(
                                                        "Cannot create timeline: {}",
                                                        create_err
                                                    ),
                                                },
                                            );
                                        }
                                    }
                                }
                            }
                        } else {
                            tracing::warn!("No active timeline set for project, selecting timeline");
                            match project_service.get_timelines().await {
                                Ok(mut timelines) if !timelines.is_empty() => {
                                    timelines.sort_by_key(|t| t.name.clone());
                                    let selected = timelines[0].id;
                                    if let Err(err) = project_service
                                        .execute(ProjectCommand::SetActiveTimeline {
                                            timeline_id: selected,
                                        })
                                        .await
                                    {
                                        send_ui_event(
                                            &tx,
                                            AppEvent::Error {
                                                message: format!(
                                                    "Failed to set active timeline: {}",
                                                    err
                                                ),
                                            },
                                        );
                                    } else if let Err(err) = timeline_service.load(selected).await {
                                        send_ui_event(
                                            &tx,
                                            AppEvent::Error {
                                                message: format!(
                                                    "Failed to load selected timeline: {}",
                                                    err
                                                ),
                                            },
                                        );
                                    }
                                }
                                _ => {
                                    let new_timeline = Timeline::new("Timeline 1").with_settings(
                                        TimelineSettings {
                                            fps: project.settings.fps,
                                            resolution: project.settings.resolution,
                                            sample_rate: project.settings.sample_rate,
                                            audio_channels: 2,
                                        },
                                    );
                                    if timeline_service
                                        .timeline_repo
                                        .create(project.id, &new_timeline)
                                        .await
                                        .is_ok()
                                    {
                                        let _ = project_service
                                            .execute(ProjectCommand::SetActiveTimeline {
                                                timeline_id: new_timeline.id,
                                            })
                                            .await;
                                        let _ = timeline_service.load(new_timeline.id).await;
                                        send_ui_event(
                                            &tx,
                                            AppEvent::TimelineCreated {
                                                timeline: new_timeline,
                                            },
                                        );
                                    } else {
                                        send_ui_event(
                                            &tx,
                                            AppEvent::Error {
                                                message: "Cannot create default timeline".into(),
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }

                    // Keep playback bounded and synced with current timeline
                    if let AppEvent::TimelineUpdated { timeline }
                    | AppEvent::TimelineCreated { timeline } = &ev
                    {
                        preview_service.update_timeline(Some(timeline.clone())).await;
                        playback_service
                            .set_max_frame(Some(timeline.duration().0))
                            .await;
                        playback_service.sync_frame(timeline.playhead.0).await;
                    }

                    if let AppEvent::ProjectClosed = &ev {
                        preview_service.update_timeline(None).await;
                        preview_service.update_assets(Vec::new()).await;
                    }

                    if let AppEvent::AssetImported { asset }
                    | AppEvent::AssetUpdated { asset }
                    | AppEvent::AssetAnalyzed { asset }
                    | AppEvent::AssetProxyComplete { asset } = &ev
                    {
                        preview_service.upsert_asset(asset.clone()).await;
                    }

                    if let AppEvent::AssetDeleted { asset_id } = &ev {
                        preview_service.remove_asset(*asset_id).await;
                    }

                    send_ui_event(&tx, ev);
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
                let open_path = p
                    .path
                    .clone()
                    .unwrap_or_else(|| data_dir.join(format!("{}-{}", sanitize_project_name(&p.name), DEFAULT_PROJECT_FILE_NAME)));
                let _ = project_service
                    .execute(ProjectCommand::Open {
                        path: open_path,
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
                    send_ui_event(
                        &evt_tx,
                        AppEvent::Error {
                            message: format!("Bootstrap project failed: {}", e),
                        },
                    );
                }
            }
        }

        // Main command loop
        while let Ok(cmd) = cmd_rx.recv_async().await {
            match cmd {
                BackendCommand::Project(c) => {
                    if let Err(e) = project_service.execute(c).await {
                        send_ui_event(
                            &evt_tx,
                            AppEvent::Error {
                                message: e.to_string(),
                            },
                        );
                    }
                }

                BackendCommand::Timeline(c) => {
                    let should_save = !matches!(c, TimelineCommand::Seek { .. });
                    if let Err(e) = timeline_service.execute(c).await {
                        send_ui_event(
                            &evt_tx,
                            AppEvent::Error {
                                message: e.to_string(),
                            },
                        );
                    } else if should_save {
                        let _ = timeline_service.save().await; // best effort auto-save
                    }
                }

                BackendCommand::Asset(c) => {
                    if let Err(e) = asset_service.execute(c).await {
                        send_ui_event(
                            &evt_tx,
                            AppEvent::Error {
                                message: e.to_string(),
                            },
                        );
                    }
                }

                BackendCommand::Playback(c) => match c {
                    PlaybackCommand::Play => playback_service.play().await,
                    PlaybackCommand::Pause => playback_service.pause().await,
                    PlaybackCommand::Stop => playback_service.stop().await,
                    PlaybackCommand::Seek { frame } => playback_service.seek(frame.0).await,
                    PlaybackCommand::SetFps { fps } => playback_service.set_fps(fps).await,
                },

                BackendCommand::Preview(c) => match c {
                    PreviewCommand::RequestFrame { frame } => {
                        preview_service.request_frame(frame).await;
                    }
                    PreviewCommand::RequestTimelineThumbnail {
                        asset_id,
                        source_frame,
                        fps,
                    } => {
                        preview_service
                            .request_timeline_thumbnail(asset_id, source_frame, fps)
                            .await;
                    }
                },

                BackendCommand::Render(c) => match c {
                    RenderCommand::PreparePlan => {
                        if let Some(timeline) = timeline_service.current().await {
                            let settings = render_service.recommended_settings(&timeline);
                            let plan = render_service.build_render_plan(&timeline, settings);
                            event_bus.emit(AppEvent::RenderPlanReady {
                                timeline_id: timeline.id,
                                plan,
                            });
                        } else {
                            send_ui_event(
                                &evt_tx,
                                AppEvent::Error {
                                    message: "No active timeline to render".into(),
                                },
                            );
                        }
                    }
                    RenderCommand::Export {
                        output_path,
                        format,
                        quality,
                        use_hardware_accel,
                    } => {
                        if let Some(timeline) = timeline_service.current().await {
                            let mut settings = render_service.recommended_settings(&timeline);
                            settings.output_path = output_path;
                            settings.format = format;
                            settings.quality = quality;
                            settings.use_hardware_accel =
                                use_hardware_accel && render_service.is_hardware_accel_available();

                            event_bus.emit(AppEvent::RenderStarted {
                                settings: settings.clone(),
                            });

                            let mut export_assets = Vec::new();
                            for clip in timeline.clips.iter().filter(|c| c.enabled) {
                                let Some(asset_id) = clip.asset_id else {
                                    continue;
                                };
                                match asset_service.get(asset_id).await {
                                    Ok(Some(asset)) => export_assets.push(asset),
                                    Ok(None) | Err(_) => {}
                                }
                            }

                            if export_assets.is_empty() {
                                event_bus.emit(AppEvent::RenderFailed {
                                    error: "No enabled clips with resolved assets to export".into(),
                                });
                                continue;
                            }

                            match render_service.export_timeline(&timeline, &export_assets, settings.clone()) {
                                Ok(result) => {
                                    event_bus.emit(AppEvent::RenderFinished { result });
                                }
                                Err(err) => {
                                    event_bus.emit(AppEvent::RenderFailed {
                                        error: err.to_string(),
                                    });
                                }
                            }
                        } else {
                            send_ui_event(
                                &evt_tx,
                                AppEvent::Error {
                                    message: "No active timeline to render".into(),
                                },
                            );
                        }
                    }
                },
            }
        }
    });
}

fn sanitize_project_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();

    let sanitized = sanitized.trim_matches('-');
    if sanitized.is_empty() {
        "untitled".to_string()
    } else {
        sanitized.to_string()
    }
}
