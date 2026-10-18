-- Replaced by belabox_start_with_stream / belabox_stop_with_stream
-- (`…_belabox_start_stop`). Ship only after the release that stopped reading it.
ALTER TABLE switchers DROP COLUMN belabox_sync_stream;
