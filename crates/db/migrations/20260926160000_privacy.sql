-- Privacy (GDPR): accounts and workspaces can be deleted, sign-in no longer
-- keeps email addresses, and old activity is cleaned up.

-- Deleting an account must not be blocked by a workspace it once created.
ALTER TABLE workspaces ALTER COLUMN created_by DROP NOT NULL;
ALTER TABLE workspaces DROP CONSTRAINT workspaces_created_by_fkey;
ALTER TABLE workspaces
    ADD CONSTRAINT workspaces_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES users ON DELETE SET NULL;

-- Data minimisation: nothing uses the sign-in email, so it's no longer asked for.
UPDATE oauth2_connections SET provider_email = NULL WHERE provider_email IS NOT NULL;

-- Only the owner may delete a workspace (and with it its switchers, history and
-- subscription).
INSERT INTO permissions (permission, description)
VALUES ('workspace.delete', 'Delete the workspace and everything in it');
INSERT INTO workspace_role_permissions (role, permission)
VALUES ('owner', 'workspace.delete');

-- The activity feed is pruned by age (see the API's retention upkeep).
CREATE INDEX switcher_events_created_at_idx ON switcher_events (created_at);

-- Sign-in tokens were stored but never used; they're no longer kept.
UPDATE oauth2_connections
SET access_token = '', refresh_token = NULL, expires_at = NULL
WHERE access_token <> '' OR refresh_token IS NOT NULL;
