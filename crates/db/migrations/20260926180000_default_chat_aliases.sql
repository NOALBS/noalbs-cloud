-- NOALBS's default chat aliases (!b bitrate, !f fix, !ss switch), which new
-- switchers now get when they're created (`db::switchers::create`). Existing
-- switchers get them too, unless they already use that alias for something.
INSERT INTO noalbs_chat_command_aliases (switcher_id, alias, command)
SELECT switchers.id, defaults.alias, defaults.command
FROM switchers
CROSS JOIN (VALUES ('b', 'bitrate'), ('f', 'fix'), ('ss', 'switch')) AS defaults (alias, command)
ON CONFLICT (switcher_id, alias) DO NOTHING;
