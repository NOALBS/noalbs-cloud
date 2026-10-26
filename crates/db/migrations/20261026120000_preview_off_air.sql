-- Who may watch the OBS preview while the switcher isn't streaming: nobody,
-- the owners (the workspace's owner, or the organization's team for a client
-- workspace), the owners and admins, or everyone who can see the switcher.
-- NOALBS support never does.
ALTER TABLE switchers
    ADD COLUMN preview_off_air TEXT NOT NULL DEFAULT 'nobody'
        CHECK (preview_off_air IN ('nobody', 'owners', 'admins', 'everyone'));
