-- An organization's own chat bot, speaking for all its client workspaces, and
-- `organizations.custom_chat_bot` as the organization's own setting: whether its
-- customers may connect a bot of their own (it was a limit staff set).
--
-- A bot belongs to one of: a workspace, an organization, or nobody (the shared
-- NOALBS bot). Which one speaks for a channel: the workspace's own (when its plan,
-- or its organization's setting, allows), else its organization's, else the shared.
--
-- Backward compatible for the moment the old services run on this schema: no
-- organization has a bot until the new API connects one.

ALTER TABLE chat_bots
    ADD COLUMN organization_id UUID REFERENCES organizations ON DELETE CASCADE;

ALTER TABLE chat_bots
    ADD CONSTRAINT chat_bots_one_owner CHECK (workspace_id IS NULL OR organization_id IS NULL);

-- One shared bot per platform, one per organization and platform, one own bot per
-- workspace and platform.
ALTER TABLE chat_bots DROP CONSTRAINT chat_bots_workspace_id_platform_key;
ALTER TABLE chat_bots
    ADD CONSTRAINT chat_bots_owner_platform_key
    UNIQUE NULLS NOT DISTINCT (workspace_id, organization_id, platform);
