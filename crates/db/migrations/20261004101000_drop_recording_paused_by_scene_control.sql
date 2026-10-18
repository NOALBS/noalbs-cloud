-- Remove the temporary recording pause ownership flag now that scene-based
-- recording control no longer uses it.
ALTER TABLE switchers
    DROP COLUMN recording_paused_by_scene_control;
