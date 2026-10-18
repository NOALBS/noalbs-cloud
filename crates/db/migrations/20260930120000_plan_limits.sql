-- How many stream servers each switcher of a workspace on the plan may have.
-- Only adding one is refused: servers already over it (after a downgrade) keep
-- working.
ALTER TABLE plans
ADD COLUMN max_stream_servers INTEGER NOT NULL DEFAULT 3 CHECK (max_stream_servers >= 0);

UPDATE plans SET max_stream_servers = 10 WHERE code = 'pro';

-- Workspace names needn't be unique, only their URL slug.
ALTER TABLE workspaces DROP CONSTRAINT workspaces_name_key;
