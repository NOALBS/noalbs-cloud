-- Recording continues only on the selected OBS scene UUIDs. An empty list
-- disables scene-based recording control.
ALTER TABLE switchers
    ADD COLUMN recording_scene_ids UUID[] NOT NULL DEFAULT '{}',
    ADD COLUMN recording_paused_by_scene_control boolean NOT NULL DEFAULT false;

-- The short-lived BRB-only option intentionally does not carry forward.
UPDATE switchers SET pause_recording_on_brb = false;
