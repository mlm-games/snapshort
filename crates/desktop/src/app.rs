use crate::state::AppState;
use crate::theme::COLORS;
use eframe::egui;
use snapshort_infra_db::DbPool;
use snapshort_usecases::{EventBus, ProjectService, TimelineService, AssetService};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::runtime::Handle;

pub struct SnapshortApp {
    runtime: Handle,
    project_service: Arc<ProjectService>,
    timeline_service: Arc<TimelineService>,
    asset_service: Arc<AssetService>,
    event_bus: EventBus,
    state: AppState,
}

impl SnapshortApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, runtime: Handle) -> Self {
        let db_path = directories::ProjectDirs::from("com", "snapshort", "editor")
            .map(|dirs| dirs.data_dir().join("snapshort.db"))
            .unwrap_or_else(|| PathBuf::from("snapshort.db"));

        let event_bus = EventBus::new();

        let db = runtime.block_on(async {
            std::fs::create_dir_all(db_path.parent().unwrap()).ok();
            DbPool::new(&db_path).await
        }).expect("Failed to initialize database");

        let proxy_dir = db_path.parent()
            .map(|p| p.join("proxies"))
            .unwrap_or_else(|| PathBuf::from("proxies"));

        let project_service = Arc::new(ProjectService::new(db.clone(), event_bus.clone()));
        let timeline_service = Arc::new(TimelineService::new(db.clone(), event_bus.clone()));
        let asset_service = Arc::new(AssetService::new(db.clone(), event_bus.clone(), proxy_dir));

        Self {
            runtime,
            project_service,
            timeline_service,
            asset_service,
            event_bus,
            state: AppState::new(),
        }
    }

    fn poll_events(&mut self) {
        while let Some(event) = self.event_bus.try_recv() {
            self.state.handle_event(event);
        }
    }
}

impl eframe::App for SnapshortApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_events();

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Project...").clicked() {
                        ui.close_menu();
                    }
                    if ui.button("Open Project...").clicked() {
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Edit", |ui| {
                    if ui.button("Undo").clicked() {
                        ui.close_menu();
                    }
                    if ui.button("Redo").clicked() {
                        ui.close_menu();
                    }
                });

                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut true, "Assets Panel");
                    ui.checkbox(&mut true, "Inspector");
                    ui.checkbox(&mut true, "Preview");
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(100.0);
                ui.heading("Welcome to Snapshort");
                ui.add_space(20.0);
                ui.label("A professional video editor built with Rust");
                ui.add_space(40.0);

                if ui.add_sized([200.0, 40.0], egui::Button::new("New Project")).clicked() {
                }

                ui.add_space(12.0);

                if ui.add_sized([200.0, 40.0], egui::Button::new("Open Project...")).clicked() {
                }
            });
        });
    }
}
