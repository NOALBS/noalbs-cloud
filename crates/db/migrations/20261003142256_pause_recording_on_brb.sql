-- Pause the recording when the BRB scene is active, and resume when it leaves.
ALTER TABLE switchers
    ADD COLUMN pause_recording_on_brb boolean NOT NULL DEFAULT false;
