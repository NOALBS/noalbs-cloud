-- Say in chat when someone picks a scene in the dashboard, in the channels that
-- keep their alerts on (`switcher_chat_channels.auto_switch_notification`).
ALTER TABLE switchers
    ADD COLUMN dashboard_switch_notification BOOLEAN NOT NULL DEFAULT true;

-- `!notify` still turns only the switcher's own switches on or off.
UPDATE noalbs_chat_commands
SET description = 'Enable/disable chat notifications of automatic switches'
WHERE command = 'notify';
