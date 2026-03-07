-- Persist project active timeline selection.

ALTER TABLE projects ADD COLUMN active_timeline_id TEXT;
