//! Native backend: tokio runtime, file stores, media services on a worker thread.
//!
//! Desktop and Android (both have threads + a filesystem). Web uses the
//! in-memory backend in `backend_wasm` instead.

#[cfg(not(target_os = "android"))]
use directories::ProjectDirs;
use flume::{Receiver, Sender};
use repose_core::request_frame;
use snapshort_infra_store::{JobStore, ProjectStore};
use snapshort_usecases::{
    AppEvent, AssetService, EventBus, JobsService, PlaybackCommand, PlaybackService,
    PreviewCommand, PreviewService, ProjectCommand, ProjectService, RenderCommand,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::state::BackendCommand;

fn send_ui_event(tx: &Sender<AppEvent>, event: AppEvent) {
    let _ = tx.send(event);
    request_frame();
}

/// A running export: the flag is polled by the encoder at its checkpoints,
/// so cancellation is cooperative and prompt without killing threads.
struct RunningExport {
    cancel: Arc<AtomicBool>,
}

fn export_in_flight(state: &Mutex<Option<RunningExport>>) -> bool {
    state.lock().map(|guard| guard.is_some()).unwrap_or(true)
}

pub fn run_backend(cmd_rx: Receiver<BackendCommand>, evt_tx: Sender<AppEvent>) {
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
        // App-private data dir: OS dirs on desktop, internal storage on
        // Android (game-utils, set from android_main like rozvp).
        #[cfg(not(target_os = "android"))]
        let Some(data_dir) = ProjectDirs::from("com", "mlm-games", "snapshort")
            .map(|d| d.data_dir().to_path_buf())
        else {
            send_ui_event(
                &evt_tx,
                AppEvent::Error {
                    message: "Failed to resolve project directories".into(),
                },
            );
            return;
        };
        #[cfg(target_os = "android")]
        let data_dir = game_utils::android_data_dir("org.mlm.snapshort");
        std::fs::create_dir_all(&data_dir).ok();

        // File-backed library: projects + job queue as JSON under the app
        // data dir (FsStorage crash-safe writes — no database init to fail).
        let proxy_dir = data_dir.join("proxies");
        std::fs::create_dir_all(&proxy_dir).ok();

        let project_store = ProjectStore::new(&data_dir);
        let job_store = JobStore::new(&data_dir);
        let event_bus = EventBus::new();
        let event_rx = event_bus.receiver();

        // Services
        let jobs = Arc::new(JobsService::new(job_store, event_bus.clone(), proxy_dir));
        jobs.recover_and_resume().await.ok();

        let project_service = Arc::new(ProjectService::new(
            project_store,
            event_bus.clone(),
            data_dir.clone(),
        ));
        let asset_service = Arc::new(AssetService::new(event_bus.clone(), jobs.clone()));
        let playback_service = Arc::new(PlaybackService::new(event_bus.clone()));
        playback_service.set_fps(24).await;

        let render_service = Arc::new(snapshort_infra_render::RenderService::new());
        let preview_service = Arc::new(PreviewService::new(
            event_bus.clone(),
            render_service.clone(),
        ));
        let export_state: Arc<Mutex<Option<RunningExport>>> = Arc::new(Mutex::new(None));

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
                    | AppEvent::ProjectOpened { project, .. } = &ev
                    {
                        let assets = project_service.list_assets().await;
                        asset_service.load_assets(assets.clone()).await;
                        // Files that vanished between sessions surface as
                        // Offline rows (with relink actions) instead of
                        // failing later as opaque job errors.
                        let assets = asset_service.mark_missing_offline().await;
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
                        if end.0 > 0 {
                            playback_service.set_max_timestamp(Some(end)).await;
                        }
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
                        // Keep the service asset set (and thus file snapshots
                        // and autosaves) in sync with media-pipeline updates.
                        project_service.add_asset(asset.clone()).await;
                        // Newly analyzed media enters the proxy policy.
                        if matches!(ev, AppEvent::AssetAnalyzed { .. }) {
                            asset_service.note_analyzed(asset.clone()).await;
                        }
                    }

                    if let AppEvent::AssetProxyComplete { asset } = &ev {
                        if let Some(proxy) = &asset.proxy {
                            preview_service
                                .set_proxy(asset.path.clone(), proxy.path.clone())
                                .await;
                        }
                    }

                    if let AppEvent::AssetDeleted { asset_id } = &ev {
                        preview_service.remove_asset_path(*asset_id).await;
                        if let Some(asset) = project_service.remove_asset(*asset_id).await {
                            preview_service.clear_proxy_for_source(&asset.path).await;
                        }
                    }

                    send_ui_event(&tx, ev);
                }
            }
        });

        // Crash-recovery shadow copy: snapshot the open project on a slow
        // cadence (web parity: 30s). Explicit saves clear it; the boot check
        // below offers whatever survives a crash.
        {
            let svc = project_service.clone();
            let dir = data_dir.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(
                    snapshort_usecases::AUTOSAVE_INTERVAL_SECS,
                ));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tick.tick().await;
                    let Some((path, snapshot)) = svc.autosave_snapshot().await else {
                        continue;
                    };
                    let meta = snapshort_usecases::AutosaveMeta {
                        project_path: path,
                        project_name: snapshot.project.meta.name.clone(),
                        saved_at_ms: snapshort_usecases::now_ms(),
                    };
                    if let Err(e) = snapshort_usecases::write_autosave(&dir, &snapshot, &meta) {
                        tracing::warn!("Autosave failed: {e}");
                    }
                }
            });
        }

        // Startup: offer crash recovery when a valid shadow copy outlives the
        // last explicit save; otherwise start with a fresh project.
        match project_service.check_autosave().await {
            Some(found) => {
                send_ui_event(
                    &evt_tx,
                    AppEvent::AutosaveFound {
                        project_name: found.project_name,
                        saved_at_ms: found.saved_at_ms,
                    },
                );
            }
            None => {
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

                BackendCommand::Edit { cmd, label } => {
                    if let Err(e) = project_service
                        .dispatch_timeline_command(cmd, label)
                        .await {
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
                    PreviewCommand::SetPreferProxy { prefer } => {
                        preview_service.set_prefer_proxy(prefer).await;
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
                        track_volumes,
                        master_volume,
                    } => {
                        if export_in_flight(&export_state) {
                            send_ui_event(
                                &evt_tx,
                                AppEvent::Error {
                                    message: "Export already in progress".into(),
                                },
                            );
                        } else if let Some(timeline) = project_service.current_timeline().await {
                            // Fail fast on missing media: relink guidance
                            // beats dying mid-encode on a vanished input.
                            let missing =
                                snapshort_usecases::missing_timeline_sources(&timeline);
                            if !missing.is_empty() {
                                let names: Vec<String> = missing
                                    .iter()
                                    .take(3)
                                    .map(|p| {
                                        p.file_name()
                                            .and_then(|n| n.to_str())
                                            .unwrap_or("?")
                                            .to_string()
                                    })
                                    .collect();
                                send_ui_event(
                                    &evt_tx,
                                    AppEvent::RenderFailed {
                                        error: format!(
                                            "{} media file(s) offline ({}…). Relink them in the Assets panel, then export again.",
                                            missing.len(),
                                            names.join(", "),
                                        ),
                                    },
                                );
                            } else {
                            let mut settings = render_service.recommended_settings(&timeline);
                            settings.output_path = output_path;
                            settings.format = format;
                            settings.quality = quality;
                            settings.use_hardware_accel =
                                use_hardware_accel && render_service.is_hardware_accel_available();

                            event_bus.emit(AppEvent::RenderStarted {
                                settings: settings.clone(),
                            });

                            // Off the command loop: encode on a blocking
                            // thread so undo, playback, and CancelExport stay
                            // live for the whole (minutes-long) run.
                            let cancel = Arc::new(AtomicBool::new(false));
                            if let Ok(mut slot) = export_state.lock() {
                                *slot = Some(RunningExport {
                                    cancel: cancel.clone(),
                                });
                            }
                            let bus = event_bus.clone();
                            let progress_bus = bus.clone();
                            let render = render_service.clone();
                            let slot = export_state.clone();
                            tokio::spawn(async move {
                                let gate = snapshort_infra_render::ProgressGate::new();
                                let result = tokio::task::spawn_blocking(move || {
                                    render.export_timeline(
                                        &timeline,
                                        &settings,
                                        &track_volumes,
                                        master_volume,
                                        &|| cancel.load(Ordering::SeqCst),
                                        &|pct| {
                                            if gate.check(
                                                pct,
                                                snapshort_infra_render::ProgressGate::now_ms(),
                                            ) {
                                                progress_bus.emit(AppEvent::RenderProgress {
                                                    percent: pct.min(100),
                                                });
                                            }
                                        },
                                    )
                                })
                                .await;
                                if let Ok(mut slot) = slot.lock() {
                                    *slot = None;
                                }
                                match result {
                                    Ok(Ok(result)) => {
                                        bus.emit(AppEvent::RenderFinished { result });
                                    }
                                    Ok(Err(err)) => {
                                        bus.emit(AppEvent::RenderFailed {
                                            error: err.to_string(),
                                        });
                                    }
                                    Err(join_err) => {
                                        bus.emit(AppEvent::RenderFailed {
                                            error: format!("Export task failed: {join_err}"),
                                        });
                                    }
                                }
                                });
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
                    RenderCommand::CancelExport => {
                        // Cooperative: the encoder observes this at its next
                        // checkpoint and reports RenderFailed("Render cancelled").
                        if let Ok(slot) = export_state.lock() {
                            if let Some(running) = slot.as_ref() {
                                running.cancel.store(true, Ordering::SeqCst);
                            }
                        }
                    }
                },
            }
        }
    });
}


