-- Starting and stopping BELABOX with the OBS stream are separate choices (some
-- want BELABOX to keep running after OBS stops). Both follow the old setting.
-- `belabox_sync_stream` stays for the release still reading it; drop it later.
ALTER TABLE switchers
    ADD COLUMN belabox_start_with_stream boolean NOT NULL DEFAULT false,
    ADD COLUMN belabox_stop_with_stream boolean NOT NULL DEFAULT false;

UPDATE switchers
SET belabox_start_with_stream = belabox_sync_stream,
    belabox_stop_with_stream = belabox_sync_stream;
