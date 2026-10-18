-- YouTube chat: channels and bots on YouTube, identified by their channel ID
-- (`UC…`). Both store Google tokens (encrypted with the data key): the channel's
-- to find its live chat and read it, the bot's to post. YouTube has no event
-- subscriptions (the chat service streams each live chat), so
-- `chat_subscriptions` stays Twitch/Kick only.
ALTER TABLE chat_bots DROP CONSTRAINT chat_bots_platform_check;
ALTER TABLE chat_bots
    ADD CONSTRAINT chat_bots_platform_check CHECK (platform IN ('TWITCH', 'KICK', 'YOUTUBE'));

ALTER TABLE chat_channels DROP CONSTRAINT chat_channels_platform_check;
ALTER TABLE chat_channels
    ADD CONSTRAINT chat_channels_platform_check CHECK (platform IN ('TWITCH', 'KICK', 'YOUTUBE'));
