use crate::{DbError, DbPool, DbResult, TimelineRepository};
use im::Vector;
use snapshort_domain::prelude::*;
use sqlx::Row;
use tracing::instrument;

#[derive(Clone)]
pub struct SqliteTimelineRepo {
    pool: DbPool,
}

impl SqliteTimelineRepo {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    async fn load_tracks(
        &self,
        timeline_id: TimelineId,
    ) -> DbResult<(Vector<Track>, Vector<Track>)> {
        tracing::debug!("Loading tracks for timeline: {}", timeline_id.0);
        let rows = sqlx::query(
            "SELECT * FROM tracks WHERE timeline_id = ? ORDER BY track_type, track_index",
        )
        .bind(timeline_id.0.to_string())
        .fetch_all(self.pool.pool())
        .await?;

        tracing::debug!(
            "Loaded {} tracks for timeline: {}",
            rows.len(),
            timeline_id.0
        );

        let mut video_tracks = Vector::new();
        let mut audio_tracks = Vector::new();

        for row in rows {
            let track_type_str: String = row.get("track_type");
            let track_type = match track_type_str.as_str() {
                "video" => TrackType::Video,
                "audio" => TrackType::Audio,
                _ => TrackType::Audio,
            };

            let track = Track {
                name: row.get("name"),
                track_type,
                index: row.get::<i32, _>("track_index") as usize, // per-type index
                locked: row.get::<i32, _>("locked") != 0,
                visible: row.get::<i32, _>("visible") != 0,
                solo: row.get::<i32, _>("solo") != 0,
                height: row.get("height"),
            };

            match track.track_type {
                TrackType::Video => video_tracks.push_back(track),
                TrackType::Audio => audio_tracks.push_back(track),
            }
        }

        Ok((video_tracks, audio_tracks))
    }

    async fn load_clips(&self, timeline_id: TimelineId) -> DbResult<Vector<Clip>> {
        tracing::debug!("Loading clips for timeline: {}", timeline_id.0);
        let rows = sqlx::query(
            "SELECT * FROM clips WHERE timeline_id = ? ORDER BY track_type, track_index, timeline_start",
        )
        .bind(timeline_id.0.to_string())
        .fetch_all(self.pool.pool())
        .await?;

        let mut clips = Vector::new();
        for row in rows {
            clips.push_back(row_to_clip(&row)?);
        }
        tracing::debug!(
            "Loaded {} clips for timeline: {}",
            clips.len(),
            timeline_id.0
        );
        Ok(clips)
    }

    #[instrument(skip(self, clips))]
    async fn save_clips(
        &self,
        timeline_id: TimelineId,
        clips: &[Clip],
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> DbResult<()> {
        sqlx::query("DELETE FROM clips WHERE timeline_id = ?")
            .bind(timeline_id.0.to_string())
            .execute(&mut **tx)
            .await?;

        for clip in clips {
            let effects_json = serde_json::to_string(&clip.effects)?;
            let clip_type_str = clip_type_to_string(&clip.clip_type);
            let track_type_str = track_type_to_string(&clip.track.track_type);

            sqlx::query(
                r#"
                INSERT INTO clips (
                    id, timeline_id, asset_id, clip_type, timeline_start,
                    track_type, track_index,
                    source_start, source_end, effects_json, name, color, enabled, locked
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(clip.id.0.to_string())
            .bind(timeline_id.0.to_string())
            .bind(clip.asset_id.map(|a| a.0.to_string()))
            .bind(clip_type_str)
            .bind(clip.timeline_start.0)
            .bind(track_type_str)
            .bind(clip.track.index as i32)
            .bind(clip.source_range.start.0)
            .bind(clip.source_range.end.0)
            .bind(&effects_json)
            .bind(&clip.name)
            .bind(&clip.color)
            .bind(clip.enabled as i32)
            .bind(clip.locked as i32)
                .execute(&mut **tx)
                .await?;
        }

        Ok(())
    }

    async fn save_tracks(
        &self,
        timeline_id: TimelineId,
        video: &[Track],
        audio: &[Track],
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> DbResult<()> {
        sqlx::query("DELETE FROM tracks WHERE timeline_id = ?")
            .bind(timeline_id.0.to_string())
            .execute(&mut **tx)
            .await?;

        for track in video {
            self.insert_track_with_index(timeline_id, track, "video", track.index, tx)
                .await?;
        }
        for track in audio {
            self.insert_track_with_index(timeline_id, track, "audio", track.index, tx)
                .await?;
        }

        Ok(())
    }

    async fn insert_track_with_index(
        &self,
        timeline_id: TimelineId,
        track: &Track,
        track_type: &str,
        index: usize,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> DbResult<()> {
        sqlx::query(
            r#"
            INSERT INTO tracks (timeline_id, name, track_type, track_index, locked, visible, solo, height)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(timeline_id.0.to_string())
        .bind(&track.name)
        .bind(track_type)
        .bind(index as i32)
        .bind(track.locked as i32)
        .bind(track.visible as i32)
        .bind(track.solo as i32)
        .bind(track.height)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}

impl TimelineRepository for SqliteTimelineRepo {
    #[instrument(skip(self, timeline))]
    async fn create(&self, project_id: ProjectId, timeline: &Timeline) -> DbResult<()> {
        tracing::info!(
            "Creating timeline '{}' (id: {}) for project: {}",
            timeline.name,
            timeline.id.0,
            project_id.0
        );
        let mut tx = self.pool.begin().await?;

        let settings_json = serde_json::to_string(&timeline.settings)?;
        let work_area_json = timeline
            .work_area
            .map(|w| serde_json::to_string(&w))
            .transpose()?;

        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO timelines (id, project_id, name, settings_json, playhead, work_area_json, created_at, modified_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(timeline.id.0.to_string())
        .bind(project_id.0.to_string())
        .bind(&timeline.name)
        .bind(&settings_json)
        .bind(timeline.playhead.0)
        .bind(&work_area_json)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        tracing::debug!("Saved timeline metadata to database");

        let video_tracks: Vec<_> = timeline.video_tracks.iter().cloned().collect();
        let audio_tracks: Vec<_> = timeline.audio_tracks.iter().cloned().collect();
        tracing::debug!(
            "Saving {} video tracks and {} audio tracks",
            video_tracks.len(),
            audio_tracks.len()
        );
        self.save_tracks(timeline.id, &video_tracks, &audio_tracks, &mut tx)
            .await?;

        let clips: Vec<_> = timeline.clips.iter().cloned().collect();
        tracing::debug!("Saving {} clips", clips.len());
        self.save_clips(timeline.id, &clips, &mut tx).await?;

        tx.commit().await?;

        tracing::info!(
            "Successfully created timeline '{}' with {} tracks and {} clips",
            timeline.name,
            video_tracks.len() + audio_tracks.len(),
            clips.len()
        );
        Ok(())
    }

    #[instrument(skip(self))]
    async fn get(&self, id: TimelineId) -> DbResult<Option<Timeline>> {
        tracing::debug!("Fetching timeline from database: {}", id.0);
        let row = sqlx::query("SELECT * FROM timelines WHERE id = ?")
            .bind(id.0.to_string())
            .fetch_optional(self.pool.pool())
            .await?;

        match row {
            Some(row) => {
                tracing::debug!("Found timeline in database: {}", id.0);
                let settings_json: String = row.get("settings_json");
                let work_area_json: Option<String> = row.get("work_area_json");
                let id_str: String = row.get("id");

                let (video_tracks, audio_tracks) = self.load_tracks(id).await?;
                let clips = self.load_clips(id).await?;

                tracing::debug!(
                    "Successfully loaded timeline '{}' from database",
                    row.get::<String, _>("name")
                );
                Ok(Some(Timeline {
                    id: TimelineId(
                        uuid::Uuid::parse_str(&id_str)
                            .map_err(|e| DbError::Constraint(format!("Invalid UUID: {}", e)))?,
                    ),
                    name: row.get("name"),
                    settings: serde_json::from_str(&settings_json)?,
                    video_tracks,
                    audio_tracks,
                    clips,
                    playhead: Frame(row.get::<i64, _>("playhead")),
                    work_area: work_area_json
                        .map(|s| serde_json::from_str(&s))
                        .transpose()?,
                }))
            }
            None => {
                tracing::warn!("Timeline not found in database: {}", id.0);
                Ok(None)
            }
        }
    }

    #[instrument(skip(self))]
    async fn get_by_project(&self, project_id: ProjectId) -> DbResult<Vec<Timeline>> {
        let timeline_rows = sqlx::query("SELECT * FROM timelines WHERE project_id = ?")
            .bind(project_id.0.to_string())
            .fetch_all(self.pool.pool())
            .await?;

        if timeline_rows.is_empty() {
            return Ok(Vec::new());
        }

        let mut timeline_ids: Vec<TimelineId> = Vec::new();
        for row in &timeline_rows {
            let id_str: String = row.get("id");
            timeline_ids.push(TimelineId(
                uuid::Uuid::parse_str(&id_str)
                    .map_err(|e| DbError::Constraint(format!("Invalid UUID: {}", e)))?,
            ));
        }

        let mut tracks_by_timeline: std::collections::HashMap<String, Vec<Track>> =
            std::collections::HashMap::new();
        let track_rows = sqlx::query(
            "SELECT * FROM tracks WHERE timeline_id IN (SELECT id FROM timelines WHERE project_id = ?) ORDER BY timeline_id, track_type, track_index",
        )
        .bind(project_id.0.to_string())
        .fetch_all(self.pool.pool())
        .await?;
        for row in track_rows {
            let tl_id: String = row.get("timeline_id");
            let track_type_str: String = row.get("track_type");
            let track_type = match track_type_str.as_str() {
                "video" => TrackType::Video,
                _ => TrackType::Audio,
            };
            let track = Track {
                name: row.get("name"),
                track_type,
                index: row.get::<i32, _>("track_index") as usize,
                locked: row.get::<i32, _>("locked") != 0,
                visible: row.get::<i32, _>("visible") != 0,
                solo: row.get::<i32, _>("solo") != 0,
                height: row.get("height"),
            };
            tracks_by_timeline.entry(tl_id).or_default().push(track);
        }

        let mut clips_by_timeline: std::collections::HashMap<String, Vec<Clip>> =
            std::collections::HashMap::new();
        let clip_rows = sqlx::query(
            "SELECT * FROM clips WHERE timeline_id IN (SELECT id FROM timelines WHERE project_id = ?) ORDER BY timeline_id, track_type, track_index, timeline_start",
        )
        .bind(project_id.0.to_string())
        .fetch_all(self.pool.pool())
        .await?;
        for row in clip_rows {
            let tl_id: String = row.get("timeline_id");
            clips_by_timeline
                .entry(tl_id)
                .or_default()
                .push(row_to_clip(&row)?);
        }

        let mut timelines = Vec::new();
        for (row, id) in timeline_rows.into_iter().zip(timeline_ids) {
            let settings_json: String = row.get("settings_json");
            let work_area_json: Option<String> = row.get("work_area_json");
            let id_str: String = row.get("id");

            let all_tracks = tracks_by_timeline.remove(&id_str).unwrap_or_default();
            let mut video_tracks = im::Vector::new();
            let mut audio_tracks = im::Vector::new();
            for t in all_tracks {
                match t.track_type {
                    TrackType::Video => video_tracks.push_back(t),
                    TrackType::Audio => audio_tracks.push_back(t),
                }
            }

            let clips: im::Vector<Clip> =
                clips_by_timeline.remove(&id_str).unwrap_or_default().into();

            timelines.push(Timeline {
                id,
                name: row.get("name"),
                settings: serde_json::from_str(&settings_json)?,
                video_tracks,
                audio_tracks,
                clips,
                playhead: Frame(row.get::<i64, _>("playhead")),
                work_area: work_area_json
                    .map(|s| serde_json::from_str(&s))
                    .transpose()?,
            });
        }

        Ok(timelines)
    }

    #[instrument(skip(self, timeline))]
    async fn update(&self, timeline: &Timeline) -> DbResult<()> {
        let mut tx = self.pool.begin().await?;

        let settings_json = serde_json::to_string(&timeline.settings)?;
        let work_area_json = timeline
            .work_area
            .map(|w| serde_json::to_string(&w))
            .transpose()?;

        let now = chrono::Utc::now().to_rfc3339();

        let result = sqlx::query(
            r#"
            UPDATE timelines SET
                name = ?, settings_json = ?, playhead = ?, work_area_json = ?, modified_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&timeline.name)
        .bind(&settings_json)
        .bind(timeline.playhead.0)
        .bind(&work_area_json)
        .bind(&now)
        .bind(timeline.id.0.to_string())
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity_type: "Timeline",
                id: timeline.id.0,
            });
        }

        let video_tracks: Vec<_> = timeline.video_tracks.iter().cloned().collect();
        let audio_tracks: Vec<_> = timeline.audio_tracks.iter().cloned().collect();
        self.save_tracks(timeline.id, &video_tracks, &audio_tracks, &mut tx)
            .await?;

        let clips: Vec<_> = timeline.clips.iter().cloned().collect();
        self.save_clips(timeline.id, &clips, &mut tx).await?;

        tx.commit().await?;

        Ok(())
    }

    #[instrument(skip(self))]
    async fn delete(&self, id: TimelineId) -> DbResult<()> {
        let result = sqlx::query("DELETE FROM timelines WHERE id = ?")
            .bind(id.0.to_string())
            .execute(self.pool.pool())
            .await?;

        if result.rows_affected() == 0 {
            return Err(DbError::NotFound {
                entity_type: "Timeline",
                id: id.0,
            });
        }

        Ok(())
    }
}

fn row_to_clip(row: &sqlx::sqlite::SqliteRow) -> DbResult<Clip> {
    let id_str: String = row.get("id");
    let asset_id_str: Option<String> = row.get("asset_id");
    let clip_type_str: String = row.get("clip_type");
    let track_type_str: String = row.get("track_type");
    let effects_json: String = row.get("effects_json");

    Ok(Clip {
        id: ClipId(
            uuid::Uuid::parse_str(&id_str)
                .map_err(|e| DbError::Constraint(format!("Invalid UUID: {}", e)))?,
        ),
        clip_type: string_to_clip_type(&clip_type_str)?,
        asset_id: asset_id_str
            .map(|s| {
                uuid::Uuid::parse_str(&s)
                    .map(AssetId)
                    .map_err(|e| DbError::Constraint(format!("Invalid UUID: {}", e)))
            })
            .transpose()?,
        timeline_start: Frame(row.get::<i64, _>("timeline_start")),
        track: TrackRef {
            track_type: string_to_track_type(&track_type_str)?,
            index: row.get::<i32, _>("track_index") as usize,
        },
        source_range: FrameRange::new_unchecked(
            row.get::<i64, _>("source_start"),
            row.get::<i64, _>("source_end"),
        ),
        effects: serde_json::from_str(&effects_json)?,
        name: row.get("name"),
        color: row.get("color"),
        enabled: row.get::<i32, _>("enabled") != 0,
        locked: row.get::<i32, _>("locked") != 0,
    })
}

fn track_type_to_string(t: &TrackType) -> &'static str {
    match t {
        TrackType::Video => "video",
        TrackType::Audio => "audio",
    }
}

fn string_to_track_type(s: &str) -> DbResult<TrackType> {
    Ok(match s {
        "video" => TrackType::Video,
        "audio" => TrackType::Audio,
        _ => return Err(DbError::Constraint(format!("Unknown track type: {}", s))),
    })
}

fn clip_type_to_string(t: &ClipType) -> &'static str {
    match t {
        ClipType::Video => "video",
        ClipType::Audio => "audio",
        ClipType::Title => "title",
        ClipType::Generator => "generator",
        ClipType::Adjustment => "adjustment",
        ClipType::Gap => "gap",
    }
}

fn string_to_clip_type(s: &str) -> DbResult<ClipType> {
    Ok(match s {
        "video" => ClipType::Video,
        "audio" => ClipType::Audio,
        "title" => ClipType::Title,
        "generator" => ClipType::Generator,
        "adjustment" => ClipType::Adjustment,
        "gap" => ClipType::Gap,
        _ => return Err(DbError::Constraint(format!("Unknown clip type: {}", s))),
    })
}
