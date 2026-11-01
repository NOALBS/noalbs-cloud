-- Show the BRB scene just before the stream stops after a raid (`auto_stop_on_raid`).
ALTER TABLE switchers
    ADD COLUMN switch_to_offline_on_raid BOOLEAN NOT NULL DEFAULT false;
