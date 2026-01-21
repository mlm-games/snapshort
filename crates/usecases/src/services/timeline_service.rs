//! Timeline service - orchestrates timeline operations
use crate::{undo_service::UndoService, AppError, AppEvent, AppResult, EventBus, TimelineCommand};
use snapshort_domain::prelude::*;
use snapshort_infra_db::{
    AssetRepository, DbPool, SqliteAssetRepo, SqliteTimelineRepo, TimelineRepository,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, instrument};

/// Service for timeline operations
pub struct TimelineService {
    pub(crate) db: DbPool,
    timeline_repo: SqliteTimelineRepo,
    asset_repo: SqliteAssetRepo,
    event_bus: EventBus,
    /// Current timeline (in-memory for fast edits)
    current: Arc<RwLock<Option<Timeline>>>,
    /// Undo service
    undo: Arc<RwLock<UndoService>>,
}

impl TimelineService {
    pub fn new(db: DbPool, event_bus: EventBus) -> Self {
        Self {
            timeline_repo: SqliteTimelineRepo::new(db.clone()),
            asset_repo: SqliteAssetRepo::new(db.clone()),
            db,
            event_bus,
            current: Arc::new(RwLock::new(None)),
            undo: Arc::new(RwLock::new(UndoService::new())),
        }
    }

    /// Load a timeline
    #[instrument(skip(self))]
    pub async fn load(&self, id: TimelineId) -> AppResult<Timeline> {
        let timeline = self
            .timeline_repo
            .get(id)
            .await?
            .ok_or(AppError::TimelineNotFound(id.0))?;

        let mut current = self.current.write().await;
        *current = Some(timeline.clone());

        let mut undo = self.undo.write().await;
        undo.init(timeline.clone());

        self.event_bus.emit(AppEvent::ActiveTimelineChanged {
            timeline_id: Some(id),
        });

        // Store listens to TimelineUpdated/Created to set UI timeline.
        self.event_bus.emit(AppEvent::TimelineUpdated {
            timeline: timeline.clone(),
        });

        Ok(timeline)
    }

    /// Get current timeline
    pub async fn current(&self) -> Option<Timeline> {
        self.current.read().await.clone()
    }

    /// Execute a timeline command
    #[instrument(skip(self))]
    pub async fn execute(&self, command: TimelineCommand) -> AppResult<()> {
        let timeline = {
            let current = self.current.read().await;
            current
                .clone()
                .ok_or(AppError::TimelineNotFound(uuid::Uuid::nil()))?
        };

        let (new_timeline, description) = match command {
            TimelineCommand::InsertClip {
                asset_id,
                track,
                timeline_start,
                source_range,
            } => {
                let asset = self
                    .asset_repo
                    .get(asset_id)
                    .await?
                    .ok_or(AppError::AssetNotFound(asset_id.0))?;

                let source = source_range.unwrap_or_else(|| {
                    asset
                        .source_range()
                        .unwrap_or(FrameRange::new_unchecked(0, 100))
                });

                let clip_type = match asset.asset_type {
                    AssetType::Video => ClipType::Video,
                    AssetType::Audio => ClipType::Audio,
                    AssetType::Image => ClipType::Video,
                    AssetType::Sequence => ClipType::Video,
                };

                let clip = Clip::from_asset(asset_id, clip_type, source, timeline_start, track);
                let new = timeline.insert_clip(clip)?;
                (new, format!("Insert clip from {}", asset.name))
            }

            TimelineCommand::RemoveClip { clip_id } => {
                let new = timeline.remove_clip(clip_id)?;
                (new, "Remove clip".to_string())
            }

            TimelineCommand::RippleDelete { clip_id } => {
                let new = timeline.ripple_delete(clip_id)?;
                (new, "Ripple delete".to_string())
            }

            TimelineCommand::MoveClip {
                clip_id,
                new_start,
                new_track,
            } => {
                let new = timeline.update_clip(clip_id, |mut clip| {
                    clip.timeline_start = new_start;
                    clip.track = new_track;
                    Ok(clip)
                })?;
                (new, "Move clip".to_string())
            }

            TimelineCommand::TrimStart { clip_id, new_start } => {
                let new = timeline.update_clip(clip_id, |mut clip| {
                    clip.trim_start(new_start)?;
                    Ok(clip)
                })?;
                (new, "Trim in-point".to_string())
            }

            TimelineCommand::TrimEnd { clip_id, new_end } => {
                let new = timeline.update_clip(clip_id, |mut clip| {
                    clip.trim_end(new_end)?;
                    Ok(clip)
                })?;
                (new, "Trim out-point".to_string())
            }

            TimelineCommand::SplitAt { clip_id, frame } => {
                // Load the clip, split it, then update left + insert right.
                let mut left = timeline.get_clip(clip_id).cloned().ok_or_else(|| {
                    AppError::Domain(DomainError::NotFound {
                        entity_type: "Clip",
                        id: clip_id.0,
                    })
                })?;

                let right = left.split_at(frame)?;

                let new = timeline
                    .update_clip(clip_id, |_| Ok(left))?
                    .insert_clip(right)?;

                (new, "Split clip".to_string())
            }

            TimelineCommand::Seek { frame } => {
                let new = timeline.seek(frame);

                // Seek doesn't add to undo history
                let mut current = self.current.write().await;
                *current = Some(new.clone());

                self.event_bus.emit(AppEvent::PlayheadMoved { frame });
                return Ok(());
            }

            TimelineCommand::AddVideoTrack => {
                let new = timeline.add_video_track();
                (new, "Add video track".to_string())
            }

            TimelineCommand::AddAudioTrack => {
                let new = timeline.add_audio_track();
                (new, "Add audio track".to_string())
            }

            TimelineCommand::SetClipSpeed { clip_id, speed } => {
                let new = timeline.update_clip(clip_id, |mut clip| {
                    clip.effects.speed = speed.clamp(0.1, 10.0);
                    Ok(clip)
                })?;
                (new, format!("Set speed to {}x", speed))
            }

            TimelineCommand::SetClipOpacity { clip_id, opacity } => {
                let new = timeline.update_clip(clip_id, |mut clip| {
                    clip.effects.opacity = opacity.clamp(0.0, 1.0);
                    Ok(clip)
                })?;
                (new, format!("Set opacity to {}%", (opacity * 100.0) as u8))
            }
        };

        // Update state
        {
            let mut current = self.current.write().await;
            *current = Some(new_timeline.clone());
        }

        // Add to undo history
        {
            let mut undo = self.undo.write().await;
            undo.push(&description, new_timeline.clone());
            self.event_bus.emit(AppEvent::UndoStackChanged {
                can_undo: undo.can_undo(),
                can_redo: undo.can_redo(),
            });
        }

        // Emit update event
        self.event_bus.emit(AppEvent::TimelineUpdated {
            timeline: new_timeline,
        });

        info!("{}", description);
        Ok(())
    }

    /// Undo last operation
    pub async fn undo(&self) -> AppResult<Option<Timeline>> {
        let timeline = {
            let mut undo = self.undo.write().await;
            let result = undo.undo();
            self.event_bus.emit(AppEvent::UndoStackChanged {
                can_undo: undo.can_undo(),
                can_redo: undo.can_redo(),
            });
            result
        };

        if let Some(ref t) = timeline {
            let mut current = self.current.write().await;
            *current = Some(t.clone());
            self.event_bus.emit(AppEvent::TimelineUpdated {
                timeline: t.clone(),
            });
        }

        Ok(timeline)
    }

    /// Redo last undone operation
    pub async fn redo(&self) -> AppResult<Option<Timeline>> {
        let timeline = {
            let mut undo = self.undo.write().await;
            let result = undo.redo();
            self.event_bus.emit(AppEvent::UndoStackChanged {
                can_undo: undo.can_undo(),
                can_redo: undo.can_redo(),
            });
            result
        };

        if let Some(ref t) = timeline {
            let mut current = self.current.write().await;
            *current = Some(t.clone());
            self.event_bus.emit(AppEvent::TimelineUpdated {
                timeline: t.clone(),
            });
        }

        Ok(timeline)
    }

    /// Save current timeline to database
    #[instrument(skip(self))]
    pub async fn save(&self) -> AppResult<()> {
        let timeline = self
            .current
            .read()
            .await
            .clone()
            .ok_or(AppError::TimelineNotFound(uuid::Uuid::nil()))?;

        self.timeline_repo.update(&timeline).await?;
        info!("Timeline saved: {}", timeline.name);
        Ok(())
    }

    pub async fn can_undo(&self) -> bool {
        self.undo.read().await.can_undo()
    }

    pub async fn can_redo(&self) -> bool {
        self.undo.read().await.can_redo()
    }
}
