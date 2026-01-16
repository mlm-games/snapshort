//! Snapshort Desktop Application

use anyhow::Result;
use eframe::egui;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod app;
mod state;
mod theme;

use app::SnapshortApp;

fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,snapshort=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Snapshort Video Editor");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("Failed to build Tokio runtime");

    let _guard = runtime.enter();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Snapshort")
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Snapshort",
        options,
        Box::new(|cc| {
            theme::setup_custom_style(&cc.egui_ctx);
            Ok(Box::new(SnapshortApp::new(cc, runtime.handle().clone())))
        }),
    )
    .expect("Failed to run eframe");
}
