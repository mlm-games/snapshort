//! Snapshort application shell (desktop + Android + web entry points).
//!
//! One crate, three mains (renamite-editor / repadio / yadaw pattern):
//! desktop and Android run the full native backend, web runs an in-memory
//! backend. File pickers are rlobkit on every platform, polled from the UI
//! pump (yadaw `Picker::poll` pattern).

pub mod pickers;
pub mod state;
pub mod views;

#[cfg(not(target_arch = "wasm32"))]
mod backend;
#[cfg(target_arch = "wasm32")]
mod backend_wasm;
#[cfg(target_arch = "wasm32")]
mod wasm_persist;

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use anyhow::Result;
#[cfg(not(target_arch = "wasm32"))]
use snapshort_usecases::AppEvent;
use std::rc::Rc;

/// Drain one picker outcome and apply it on the UI thread. Paths dispatch
/// backend commands; the web-only bytes arms go to the in-memory backend.
fn apply_picker_outcome(store: &Rc<state::Store>, outcome: pickers::CompletedPicker) {
    use pickers::CompletedPicker;
    match outcome {
        CompletedPicker::OpenPath(path) => {
            store.dispatch_project(snapshort_usecases::ProjectCommand::Open { path });
        }
        CompletedPicker::ImportPaths(paths) => {
            store.dispatch_asset(snapshort_usecases::AssetCommand::Import { paths });
        }
        CompletedPicker::SavePath { path, markers } => {
            store.dispatch_project(snapshort_usecases::ProjectCommand::SaveAs { path, markers });
        }
        CompletedPicker::ExportPath(path) => {
            store.state.export_output_path.set(Some(path));
            store
                .state
                .status_msg
                .set("Export output selected.".into());
        }
        CompletedPicker::SaveDownload { .. }
        | CompletedPicker::OpenBytes { .. }
        | CompletedPicker::ImportBytes(_) => {
            #[cfg(target_arch = "wasm32")]
            {
                // Handled by the wasm pump, which owns the backend.
                store.state.status_msg.set(
                    "Internal error: web picker outcome reached the shared applier.".into(),
                );
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                store
                    .state
                    .status_msg
                    .set("Download-style picker outcome is web-only.".into());
            }
        }
        CompletedPicker::Cancelled => {}
        CompletedPicker::Failed(e) => {
            store.state.status_msg.set(format!("Picker failed: {e}"));
        }
    }
}

/// Desktop entry: logging + native backend thread + desktop window.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn desktop_main() -> Result<()> {
    use tracing_subscriber::prelude::*;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info,snapshort=debug"))
        .with(tracing_subscriber::fmt::layer())
        .init();
    rlobkit_dialogs::init();

    let (cmd_tx, cmd_rx) = flume::unbounded::<state::BackendCommand>();
    let (evt_tx, evt_rx) = flume::unbounded::<AppEvent>();

    let dock_state = views::panels::create_default_layout();
    let store = Rc::new(state::Store::new(cmd_tx, dock_state));

    std::thread::spawn(move || backend::run_backend(cmd_rx, evt_tx));

    let config = repose_platform::AppConfig {
        window_title: "Snapshort".to_string(),
        ..Default::default()
    };
    repose_platform::run_desktop_app_with_config(
        move |_sched, ctx| {
            store.ensure_render_context(ctx);
            while let Ok(event) = evt_rx.try_recv() {
                store.handle_event(event);
            }
            if let Some(outcome) = pickers::drain_picker(&store) {
                apply_picker_outcome(&store, outcome);
            }
            views::root_view(store.clone())
        },
        config,
    )?;
    Ok(())
}

/// Android entry: native backend on a worker thread, app-private storage.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "C" fn android_main(android_app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );
    rlobkit_dialogs::init_shared_pending_state();
    rlobkit_dialogs::init_with_android_context(
        android_app.vm_as_ptr().cast(),
        android_app.activity_as_ptr().cast(),
    );
    rlobkit_dialogs::init();
    log::info!(
        "rlobkit helper activity available: {}",
        rlobkit_dialogs::helper_activity_available_for_host()
    );
    if let Some(dir) = android_app.internal_data_path() {
        game_utils::set_android_data_dir(dir.join("files"));
    }

    let (cmd_tx, cmd_rx) = flume::unbounded::<state::BackendCommand>();
    let (evt_tx, evt_rx) = flume::unbounded::<AppEvent>();

    let dock_state = views::panels::create_default_layout();
    let store = Rc::new(state::Store::new(cmd_tx, dock_state));

    std::thread::spawn(move || backend::run_backend(cmd_rx, evt_tx));

    let _ = repose_platform::android::run_android_app_with_options(
        android_app,
        move |_sched, ctx| {
            store.ensure_render_context(ctx);
            while let Ok(event) = evt_rx.try_recv() {
                store.handle_event(event);
            }
            if let Some(outcome) = pickers::drain_picker(&store) {
                apply_picker_outcome(&store, outcome);
            }
            views::root_view(store.clone())
        },
        Default::default(),
    );
}

/// Web entry (`cdylib`, loaded by Trunk): in-memory backend over OPFS
/// project storage, JSON up/download for file interchange.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_start() -> Result<(), wasm_bindgen::prelude::JsValue> {
    use pickers::CompletedPicker;

    rlobkit_dialogs::init();

    let (cmd_tx, cmd_rx) = flume::unbounded::<state::BackendCommand>();
    let (restore_tx, restore_rx) = flume::unbounded::<Vec<u8>>();
    let dock_state = views::panels::create_default_layout();
    let store = Rc::new(state::Store::new(cmd_tx.clone(), dock_state));
    let mut backend = backend_wasm::WasmBackend::new(restore_rx);

    // Boot project immediately; an autosave restore overwrites it if present.
    cmd_tx
        .send(state::BackendCommand::Project(
            snapshort_usecases::ProjectCommand::Create {
                name: "Untitled".to_string(),
            },
        ))
        .ok();
    web_workers::spawn_async_unified(move || async move {
        if wasm_persist::init().await.is_err() {
            return;
        }
        if let Some(bytes) = wasm_persist::load_autosave().await {
            let _ = restore_tx.send(bytes);
        }
    });

    let options = repose_platform::web::WebOptions::new(None);
    repose_platform::web::run_web_app(
        move |_sched, ctx| {
            store.ensure_render_context(ctx);
            backend.drain(&store, &cmd_rx);
            if let Some(outcome) = pickers::drain_picker(&store) {
                match outcome {
                    CompletedPicker::OpenBytes { name, data } => {
                        backend.open_json(&store, name, &data)
                    }
                    CompletedPicker::ImportBytes(files) => {
                        for (name, data) in files {
                            backend.ingest_media_bytes(&store, name, data);
                        }
                    }
                    CompletedPicker::SaveDownload { name, markers } => {
                        backend.save_download(&store, name, markers)
                    }
                    other => apply_picker_outcome(&store, other),
                }
            }
            views::root_view(store.clone())
        },
        options,
    )
}
