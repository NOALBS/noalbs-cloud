-- BELABOX Cloud: the encoder's remote key (encrypted with the data key, like
-- the OBS password; NULL = not used) and whether BELABOX starts and stops with
-- the OBS stream.
ALTER TABLE switchers
    ADD COLUMN belabox_remote_key BYTEA,
    ADD COLUMN belabox_sync_stream boolean NOT NULL DEFAULT false;

-- belabot's commands, for switchers with a remote key.
INSERT INTO noalbs_chat_commands (command, default_role, description)
VALUES
    ('bbstart',  'ADMIN',  'Start the BELABOX stream'),
    ('bbstop',   'ADMIN',  'Stop the BELABOX stream'),
    ('bbb',      'ADMIN',  'Set the BELABOX max bitrate'),
    ('bbt',      'ADMIN',  'Turn a BELABOX network interface on or off'),
    ('bbs',      'PUBLIC', 'Report the BELABOX network interfaces and bitrate'),
    ('bbsensor', 'PUBLIC', 'Report the BELABOX sensors');

-- belabot's !bbm (modems) says the same as !bbs. New switchers get it from
-- `default_aliases!` in `db::switchers`.
INSERT INTO noalbs_chat_command_aliases (switcher_id, alias, command)
SELECT id, 'bbm', 'bbs' FROM switchers
ON CONFLICT (switcher_id, alias) DO NOTHING;
