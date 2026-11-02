-- The newest "What's new" post the user has seen (its ID, the file name without
-- `.md`, e.g. `2026-11-02-scene-picker`), so the dashboard only shows newer
-- ones. The posts live in the repo; IDs sort by date. NULL: none seen yet.
ALTER TABLE users
ADD COLUMN whats_new_seen text CHECK (char_length(whats_new_seen) BETWEEN 1 AND 100);
