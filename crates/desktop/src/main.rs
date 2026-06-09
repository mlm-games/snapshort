use anyhow::Result;
use directories::ProjectDirs;
use flume::{Receiver, Sender};
use repose_core::request_frame;
use repose_platform::run_desktop_app;
use snapshort_infra_db::DbConn;
use snapshort_usecases::{
    AppEvent, AssetService, EventBus, JobsService, PlaybackCommand, PlaybackService,
    PreviewCommand, PreviewService, ProjectCommand, ProjectService, RenderCommand,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
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
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            let _ = evt_tx.send(AppEvent::Error {
                message: format!("Failed to build async runtime: {e}"),
            });
            return;
        }
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

        let conn = match DbConn::new(&db_path).await {
            Ok(conn) => conn,
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
        let jobs = Arc::new(JobsService::new(conn.clone(), event_bus.clone(), proxy_dir));
        jobs.recover_and_resume().await.ok();

        let project_service = Arc::new(ProjectService::new(conn, event_bus.clone()));
        let asset_service = Arc::new(AssetService::new(event_bus.clone(), jobs.clone()));
        let playback_service = Arc::new(PlaybackService::new(event_bus.clone()));
        playback_service.set_fps(24).await;

        let render_service = Arc::new(snapshort_infra_render::RenderService::new());
        let preview_service = Arc::new(PreviewService::new(
            event_bus.clone(),
            render_service.clone(),
        ));

        // Forwarder: event bus -> UI flume + orchestration hooks
        tokio::spawn({
            let tx = evt_tx.clone();
            let project_service = project_service.clone();
            let asset_service = asset_service.clone();
            let playback_service = playback_service.clone();
            let preview_service = preview_service.clone();

            async move {
                while let Ok(ev) = event_rx.recv_async().await {
                    // On project created/opened: load assets into services
                    if let AppEvent::ProjectCreated { project }
                    | AppEvent::ProjectOpened { project } = &ev
                    {
                        let assets = project_service.list_assets().await;
                        asset_service.load_assets(assets.clone()).await;
                        jobs.load_assets(assets.clone()).await;
                        let path_map: HashMap<_, _> = assets
                            .iter()
                            .map(|a| (a.id, a.effective_path().clone()))
                            .collect();
                        preview_service.update_asset_paths(path_map).await;
                    }

                    // Sync playback bounds on timeline changes
                    if let AppEvent::TimelineUpdated { timeline } = &ev
                    {
                        preview_service.update_timeline(Some(timeline.clone())).await;
                        let end = timeline.duration_end();
                        playback_service.set_max_timestamp(Some(end)).await;
                    }

                    if let AppEvent::ProjectClosed = &ev {
                        preview_service.update_timeline(None).await;
                        preview_service
                            .update_asset_paths(HashMap::new())
                            .await;
                    }

                    if let AppEvent::AssetImported { asset }
                    | AppEvent::AssetUpdated { asset }
                    | AppEvent::AssetAnalyzed { asset }
                    | AppEvent::AssetProxyComplete { asset } = &ev
                    {
                        preview_service
                            .upsert_asset_path(asset.id, asset.effective_path().clone())
                            .await;
                    }

                    if let AppEvent::AssetDeleted { asset_id } = &ev {
                        preview_service.remove_asset_path(*asset_id).await;
                    }

                    send_ui_event(&tx, ev);
                }
            }
        });

        // Startup: create a new project (DB-stored project loading TBD)
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

                BackendCommand::Edit(c) => {
                    if let Err(e) = project_service.dispatch_timeline_command(c).await {
                        send_ui_event(
                            &evt_tx,
                            AppEvent::Error {
                                message: e.to_string(),
                            },
                        );
                    }
                }

                BackendCommand::Undo => {
                    if let Err(e) = project_service.undo_timeline().await {
                        send_ui_event(
                            &evt_tx,
                            AppEvent::Error {
                                message: e.to_string(),
                            },
                        );
                    }
                }

                BackendCommand::Redo => {
                    if let Err(e) = project_service.redo_timeline().await {
                        send_ui_event(
                            &evt_tx,
                            AppEvent::Error {
                                message: e.to_string(),
                            },
                        );
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
                    PlaybackCommand::Seek { timestamp } => {
                        playback_service.seek(timestamp).await;
                        project_service.set_playhead(timestamp).await;
                    }
                    PlaybackCommand::SetFps { fps } => playback_service.set_fps(fps).await,
                },

                BackendCommand::Preview(c) => match c {
                    PreviewCommand::RequestFrame { timestamp } => {
                        preview_service.request_frame(timestamp).await;
                    }
                    PreviewCommand::RequestTimelineThumbnail {
                        asset_id,
                        source_time,
                    } => {
                        preview_service
                            .request_timeline_thumbnail(asset_id, source_time)
                            .await;
                    }
                },

                BackendCommand::Render(c) => match c {
                    RenderCommand::PreparePlan => {
                        if let Some(timeline) = project_service.current_timeline().await {
                            let settings = render_service.recommended_settings(&timeline);
                            let plan = render_service.build_render_plan(&timeline, settings);
                            event_bus.emit(AppEvent::RenderPlanReady { plan });
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
                        if let Some(timeline) = project_service.current_timeline().await {
                            let mut settings = render_service.recommended_settings(&timeline);
                            settings.output_path = output_path;
                            settings.format = format;
                            settings.quality = quality;
                            settings.use_hardware_accel =
                                use_hardware_accel && render_service.is_hardware_accel_available();

                            event_bus.emit(AppEvent::RenderStarted {
                                settings: settings.clone(),
                            });

                            match render_service.export_timeline(&timeline, settings.clone()) {
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
