-- Automatic switch notifications per linked channel: a switcher with
-- `auto_switch_notification` on posts only in the channels that keep this on.
ALTER TABLE switcher_chat_channels
    ADD COLUMN auto_switch_notification BOOLEAN NOT NULL DEFAULT true;
