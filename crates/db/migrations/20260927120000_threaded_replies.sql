-- Whether the bot answers a command as a reply to its message (Twitch/Kick's
-- reply thread) or as a plain chat message.
ALTER TABLE switchers ADD COLUMN threaded_replies boolean NOT NULL DEFAULT true;
