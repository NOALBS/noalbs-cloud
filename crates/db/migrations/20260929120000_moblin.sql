-- Moblin remote control: the phone connects to obs-link as a remote control
-- "streamer" with this password (encrypted with the data key, like the OBS
-- password; NULL = Moblin isn't used), and may start and stop with the OBS stream.
ALTER TABLE switchers
    ADD COLUMN moblin_password BYTEA,
    ADD COLUMN moblin_start_with_stream boolean NOT NULL DEFAULT false,
    ADD COLUMN moblin_stop_with_stream boolean NOT NULL DEFAULT false;

-- Chat commands for switchers with a Moblin password.
INSERT INTO noalbs_chat_commands (command, default_role, description)
VALUES
    ('mstart',   'ADMIN',  'Start the Moblin stream'),
    ('mstop',    'ADMIN',  'Stop the Moblin stream'),
    ('mbitrate', 'ADMIN',  'Pick a Moblin bitrate preset'),
    ('mscene',   'ADMIN',  'Switch the Moblin scene'),
    ('mstats',   'PUBLIC', 'Report the Moblin battery, bitrate and bonding');
