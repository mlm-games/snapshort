-- Fix track indexing semantics: tracks are unique per (timeline_id, track_type, track_index)
-- and clips store (track_type, track_index) instead of a mixed global index.

PRAGMA foreign_keys=OFF;

-- -------------------------
-- Tracks
-- -------------------------
CREATE TABLE IF NOT EXISTS tracks_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timeline_id TEXT NOT NULL REFERENCES timelines(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    track_type TEXT NOT NULL,
    track_index INTEGER NOT NULL,
    locked INTEGER NOT NULL DEFAULT 0,
    visible INTEGER NOT NULL DEFAULT 1,
    solo INTEGER NOT NULL DEFAULT 0,
    height REAL NOT NULL DEFAULT 60.0
);

-- Convert old global index -> per-type index
INSERT INTO tracks_new (timeline_id, name, track_type, track_index, locked, visible, solo, height)
SELECT
    t1.timeline_id,
    t1.name,
    t1.track_type,
    (
      SELECT COUNT(*)
      FROM tracks t2
      WHERE t2.timeline_id = t1.timeline_id
        AND t2.track_type = t1.track_type
        AND t2.track_index < t1.track_index
    ) AS track_index,
    t1.locked,
    t1.visible,
    t1.solo,
    t1.height
FROM tracks t1;

DROP TABLE tracks;
ALTER TABLE tracks_new RENAME TO tracks;

DROP INDEX IF EXISTS idx_tracks_unique;
DROP INDEX IF EXISTS idx_tracks_timeline;
CREATE INDEX IF NOT EXISTS idx_tracks_timeline ON tracks(timeline_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_tracks_unique ON tracks(timeline_id, track_type, track_index);

-- -------------------------
-- Clips
-- -------------------------
CREATE TABLE IF NOT EXISTS clips_new (
    id TEXT PRIMARY KEY NOT NULL,
    timeline_id TEXT NOT NULL REFERENCES timelines(id) ON DELETE CASCADE,
    asset_id TEXT REFERENCES assets(id) ON DELETE SET NULL,
    clip_type TEXT NOT NULL,
    timeline_start INTEGER NOT NULL,

    track_type TEXT NOT NULL,
    track_index INTEGER NOT NULL,

    source_start INTEGER NOT NULL,
    source_end INTEGER NOT NULL,
    effects_json TEXT NOT NULL DEFAULT '{}',
    name TEXT,
    color TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    locked INTEGER NOT NULL DEFAULT 0
);

-- Old behavior assumed: global track_index = [video tracks..., audio tracks...]
INSERT INTO clips_new (
    id, timeline_id, asset_id, clip_type, timeline_start,
    track_type, track_index,
    source_start, source_end, effects_json, name, color, enabled, locked
)
SELECT
    c.id,
    c.timeline_id,
    c.asset_id,
    c.clip_type,
    c.timeline_start,

    CASE
      WHEN c.track_index < (
        SELECT COUNT(*) FROM tracks t
        WHERE t.timeline_id = c.timeline_id AND t.track_type = 'video'
      )
      THEN 'video'
      ELSE 'audio'
    END AS track_type,

    CASE
      WHEN c.track_index < (
        SELECT COUNT(*) FROM tracks t
        WHERE t.timeline_id = c.timeline_id AND t.track_type = 'video'
      )
      THEN c.track_index
      ELSE c.track_index - (
        SELECT COUNT(*) FROM tracks t
        WHERE t.timeline_id = c.timeline_id AND t.track_type = 'video'
      )
    END AS track_index,

    c.source_start,
    c.source_end,
    c.effects_json,
    c.name,
    c.color,
    c.enabled,
    c.locked
FROM clips c;

DROP TABLE clips;
ALTER TABLE clips_new RENAME TO clips;

DROP INDEX IF EXISTS idx_clips_timeline;
DROP INDEX IF EXISTS idx_clips_asset;
DROP INDEX IF EXISTS idx_clips_track;
CREATE INDEX IF NOT EXISTS idx_clips_timeline ON clips(timeline_id);
CREATE INDEX IF NOT EXISTS idx_clips_asset ON clips(asset_id);
CREATE INDEX IF NOT EXISTS idx_clips_track ON clips(timeline_id, track_type, track_index);

PRAGMA foreign_keys=ON;
