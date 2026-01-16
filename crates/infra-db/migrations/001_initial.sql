-- Initial schema for Snapshort

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    path TEXT,
    settings_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    modified_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS assets (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    path TEXT NOT NULL,
    asset_type TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    media_info_json TEXT,
    proxy_json TEXT,
    imported_at TEXT NOT NULL,
    modified_at TEXT NOT NULL,
    tags_json TEXT NOT NULL DEFAULT '[]',
    notes TEXT,
    rating INTEGER,
    markers_json TEXT NOT NULL DEFAULT '[]'
);

CREATE INDEX IF NOT EXISTS idx_assets_project ON assets(project_id);
CREATE INDEX IF NOT EXISTS idx_assets_status ON assets(status);

CREATE TABLE IF NOT EXISTS timelines (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    settings_json TEXT NOT NULL,
    playhead INTEGER NOT NULL DEFAULT 0,
    work_area_json TEXT,
    created_at TEXT NOT NULL,
    modified_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_timelines_project ON timelines(project_id);

CREATE TABLE IF NOT EXISTS tracks (
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

CREATE INDEX IF NOT EXISTS idx_tracks_timeline ON tracks(timeline_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_tracks_unique ON tracks(timeline_id, track_index);

CREATE TABLE IF NOT EXISTS clips (
    id TEXT PRIMARY KEY NOT NULL,
    timeline_id TEXT NOT NULL REFERENCES timelines(id) ON DELETE CASCADE,
    asset_id TEXT REFERENCES assets(id) ON DELETE SET NULL,
    clip_type TEXT NOT NULL,
    timeline_start INTEGER NOT NULL,
    track_index INTEGER NOT NULL,
    source_start INTEGER NOT NULL,
    source_end INTEGER NOT NULL,
    effects_json TEXT NOT NULL DEFAULT '{}',
    name TEXT,
    color TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    locked INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_clips_timeline ON clips(timeline_id);
CREATE INDEX IF NOT EXISTS idx_clips_asset ON clips(asset_id);
CREATE INDEX IF NOT EXISTS idx_clips_track ON clips(timeline_id, track_index);

CREATE TABLE IF NOT EXISTS undo_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    action_type TEXT NOT NULL,
    action_data_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    undone INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_undo_project ON undo_history(project_id, created_at DESC);
