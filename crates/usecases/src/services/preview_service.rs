use crate::{AppEvent, EventBus};
use snapshort_domain::prelude::*;
use snapshort_infra_render::RenderService;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct PreviewService {
    event_bus: EventBus,
    renderer: Arc<RenderService>,
    timeline: Arc<RwLock<Option<Timeline>>>,
    assets: Arc<RwLock<Vec<Asset>>>,
    cache: Arc<RwLock<HashMap<(TimelineId, i64), Vec<u8>>>>,
}

impl PreviewService {
    pub fn new(event_bus: EventBus, renderer: Arc<RenderService>) -> Self {
        Self {
            event_bus,
            renderer,
            timeline: Arc::new(RwLock::new(None)),
            assets: Arc::new(RwLock::new(Vec::new())),
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn update_timeline(&self, timeline: Option<Timeline>) {
        *self.timeline.write().await = timeline;
    }

    pub async fn update_assets(&self, assets: Vec<Asset>) {
        *self.assets.write().await = assets;
        self.cache.write().await.clear();
    }

    pub async fn request_frame(&self, frame: Frame) {
        let Some(timeline) = self.timeline.read().await.clone() else {
            return;
        };

        let key = (timeline.id, frame.0);
        if let Some(bytes) = self.cache.read().await.get(&key).cloned() {
            self.event_bus.emit(AppEvent::PreviewFrameReady { frame, png_bytes: bytes });
            return;
        }

        let assets = self.assets.read().await.clone();
        let renderer = self.renderer.clone();
        let cache = self.cache.clone();
        let event_bus = self.event_bus.clone();

        tokio::task::spawn_blocking(move || renderer.render_preview_frame(&timeline, &assets, frame))
            .await
            .map_err(|err| err.to_string())
            .and_then(|result| result.map_err(|err| err.to_string()))
            .map_or_else(
                |error| {
                    event_bus.emit(AppEvent::PreviewFrameFailed { frame, error });
                },
                |bytes| {
                    let cache = cache.clone();
                    let event_bus = event_bus.clone();
                    tokio::spawn(async move {
                        cache.write().await.insert(key, bytes.clone());
                        event_bus.emit(AppEvent::PreviewFrameReady { frame, png_bytes: bytes });
                    });
                },
            );
    }
}
