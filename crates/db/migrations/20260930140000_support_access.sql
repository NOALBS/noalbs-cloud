-- Support access: a workspace lets NOALBS staff in for a while, and sees what
-- staff did there.
INSERT INTO permissions (permission, description)
VALUES ('workspace.support_access', 'Allow NOALBS support into the workspace and see what it did');
INSERT INTO workspace_role_permissions (role, permission)
VALUES ('owner', 'workspace.support_access'), ('admin', 'workspace.support_access');

-- `view` sees switchers, settings and history; `control` can also act on them.
CREATE TYPE support_access_level AS ENUM ('view', 'control');

-- A grant ends at `expires_at` or when revoked; at most one is active per
-- workspace (a new one revokes the previous).
CREATE TABLE support_grants (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces ON DELETE CASCADE,
    level support_access_level NOT NULL,
    -- `NULL` once that account is deleted.
    granted_by UUID REFERENCES users ON DELETE SET NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX support_grants_workspace_id_idx ON support_grants (workspace_id, created_at DESC);

CREATE TRIGGER support_grants_set_updated_at
BEFORE UPDATE ON support_grants FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- Every request staff made in a workspace they aren't a member of (reads at
-- most once an hour per route). Append-only.
CREATE TABLE support_access_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces ON DELETE CASCADE,
    -- The grant that allowed it; `NULL` = without the workspace's permission.
    grant_id UUID REFERENCES support_grants ON DELETE SET NULL,
    staff_user_id UUID REFERENCES users ON DELETE SET NULL,
    -- As they were called then, so the entry outlives their account.
    staff_name TEXT NOT NULL,
    method TEXT NOT NULL,
    -- The route template (`/workspaces/{workspace_id}/…`), not the URL.
    route TEXT NOT NULL,
    switcher_id UUID,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX support_access_log_workspace_id_idx ON support_access_log (workspace_id, created_at DESC);
