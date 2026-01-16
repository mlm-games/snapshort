use snapshort_usecases::AppEvent;

#[derive(Default)]
pub struct AppState {
    pub is_dirty: bool,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::ProjectCreated { .. } | AppEvent::ProjectOpened { .. } => {
                self.is_dirty = false;
            }
            AppEvent::ProjectSaved { .. } => {
                self.is_dirty = false;
            }
            _ => {}
        }
    }
}
