//! Native backend: one tokio runtime running the session, plus the control
//! channel an agent drives the live app over.
//!
//! Desktop and Android (both have threads + a filesystem). Web uses the
//! in-memory backend in `backend_wasm` instead.

use base64::Engine as _;
#[cfg(not(target_os = "android"))]
use directories::ProjectDirs;
use snapshort_automation::{ControlFuture, ControlHandler, ControlServer};
use snapshort_engine::Session;
use snapshort_usecases::AppEvent;
use std::sync::Arc;

use crate::control::UiChannel;
use crate::state::BackendCommand;

/// The app data dir: OS dirs on desktop, internal storage on Android.
pub fn app_data_dir() -> Option<std::path::PathBuf> {
    #[cfg(not(target_os = "android"))]
    {
        ProjectDirs::from("com", "mlm-games", "snapshort").map(|d| d.data_dir().to_path_buf())
    }
    #[cfg(target_os = "android")]
    {
        Some(game_utils::android_data_dir("org.mlm.snapshort"))
    }
}

/// Engine commands run on the session; UI actions are forwarded to the app's
/// UI thread, except screenshots, which the session renders itself.
struct AppControl {
    session: Arc<Session>,
    ui: UiChannel,
}

impl ControlHandler for AppControl {
    fn engine_execute<'a>(
        &'a self,
        command: &'a str,
        params: serde_json::Value,
    ) -> ControlFuture<'a> {
        Box::pin(async move {
            self.session
                .execute(command, params)
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn ui<'a>(&'a self, method: &'a str, params: serde_json::Value) -> ControlFuture<'a> {
        Box::pin(async move {
            let method = method.to_string();
            if method == "ui.screenshot" {
                let max_side = params
                    .get("maxSide")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(1280) as u32;
                let at = params
                    .get("seconds")
                    .and_then(serde_json::Value::as_f64)
                    .map(miniter_domain::Timestamp::from_secs)
                    .or_else(|| {
                        params
                            .get("us")
                            .and_then(serde_json::Value::as_i64)
                            .map(miniter_domain::Timestamp::from_micros)
                    });
                let png = self
                    .session
                    .render_frame_png(at, max_side)
                    .await
                    .map_err(|e| e.to_string())?;
                return Ok(serde_json::json!({
                    "png": base64::engine::general_purpose::STANDARD.encode(&png),
                    "bytes": png.len(),
                }));
            }
            let ui = self.ui.clone();
            tokio::task::spawn_blocking(move || ui.request(&method, params))
                .await
                .unwrap_or_else(|e| Err(format!("ui request failed: {e}")))
        })
    }
}

pub fn run_backend(
    cmd_rx: flume::Receiver<BackendCommand>,
    evt_tx: flume::Sender<AppEvent>,
    control_port: Option<u16>,
    ui: UiChannel,
) {
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
        let Some(data_dir) = app_data_dir() else {
            let _ = evt_tx.send(AppEvent::Error {
                message: "Failed to resolve project directories".into(),
            });
            return;
        };
        std::fs::create_dir_all(&data_dir).ok();

        let session = Arc::new(Session::new(data_dir));
        if let Err(e) = session.start().await {
            let _ = evt_tx.send(AppEvent::Error {
                message: format!("Backend start failed: {e}"),
            });
            return;
        }

        if let Some(port) = control_port {
            let handler = Arc::new(AppControl {
                session: session.clone(),
                ui,
            });
            tokio::spawn(async move {
                if let Err(e) = ControlServer::new(handler)
                    .serve(&format!("127.0.0.1:{port}"))
                    .await
                {
                    tracing::error!("Control channel failed: {e}");
                }
            });
        }

        let evt_rx = session.events();
        let tx = evt_tx.clone();
        tokio::spawn(async move {
            while let Ok(ev) = evt_rx.recv_async().await {
                if tx.send(ev).is_err() {
                    break;
                }
            }
        });

        session.serve(cmd_rx).await;
    });
}
