-- Say what !fix and !refresh each do (they shared "Try to fix the stream"), and
-- drop the host from !autostop: only a raid stops the stream.
UPDATE noalbs_chat_commands
SET description = 'Reconnect the stream sources on the scene on air'
WHERE command = 'fix';

UPDATE noalbs_chat_commands
SET description = 'Show the Refresh scene for a moment, then switch back'
WHERE command = 'refresh';

UPDATE noalbs_chat_commands
SET description = 'Enable/disable stopping the stream on a raid'
WHERE command = 'autostop';
