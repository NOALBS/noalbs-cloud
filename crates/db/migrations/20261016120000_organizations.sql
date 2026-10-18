-- Organizations: a company that manages workspaces for its customers ("clients").
-- Staff create them and set the pooled limits (comped: there is no Stripe plan for
-- an organization yet). A client workspace belongs to an organization, has no owner
-- member (the organization is the owner) and is limited by the organization instead
-- of a plan; the customer joins it as an admin member through an invite.

CREATE TABLE organizations (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    url_slug text NOT NULL UNIQUE,

    -- The pool: switchers in all of its workspaces together.
    max_switchers integer NOT NULL CHECK (max_switchers >= 0),
    -- The rest apply to every client workspace (per switcher / per workspace).
    max_stream_servers integer NOT NULL CHECK (max_stream_servers >= 0),
    max_workspace_members integer NOT NULL CHECK (max_workspace_members >= 0),
    history_days integer NOT NULL CHECK (history_days >= 0),
    custom_chat_bot boolean NOT NULL DEFAULT false,

    -- Staff can suspend it: every switcher of its workspaces is paused.
    suspended_at timestamptz,
    suspended_reason text,

    created_by uuid REFERENCES users ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TRIGGER organizations_set_updated_at
BEFORE UPDATE ON organizations FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

CREATE TABLE organization_roles (
    role text PRIMARY KEY,
    rank smallint NOT NULL UNIQUE
);

INSERT INTO organization_roles (role, rank)
VALUES ('member', 0), ('admin', 10);

INSERT INTO permissions (permission, description)
VALUES
    ('org.view', 'See the organization, its clients and its pool'),
    ('org.manage', 'Create, change and delete the organization''s client workspaces'),
    ('org.members', 'Invite and remove the organization''s team');

-- What an organization role may do, in the organization and in every client
-- workspace of it (the workspace permissions it gets there). Never billing or
-- transfer: there is nothing to pay for or hand over in a client workspace.
CREATE TABLE organization_role_permissions (
    role text NOT NULL REFERENCES organization_roles (role) ON UPDATE CASCADE ON DELETE CASCADE,
    permission text NOT NULL REFERENCES permissions (permission) ON UPDATE CASCADE ON DELETE CASCADE,
    PRIMARY KEY (role, permission)
);

INSERT INTO organization_role_permissions (role, permission)
VALUES
    ('member', 'org.view'),
    ('member', 'account.view'),
    ('member', 'account.manage'),
    ('member', 'switcher.view'),
    ('member', 'switcher.control'),
    ('member', 'switcher.manage'),
    ('admin', 'org.view'),
    ('admin', 'org.manage'),
    ('admin', 'org.members'),
    ('admin', 'account.view'),
    ('admin', 'account.manage'),
    ('admin', 'switcher.view'),
    ('admin', 'switcher.control'),
    ('admin', 'switcher.manage'),
    ('admin', 'workspace.delete'),
    ('admin', 'workspace.support_access');

CREATE TABLE organization_members (
    organization_id uuid NOT NULL REFERENCES organizations ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users ON DELETE CASCADE,
    role text NOT NULL REFERENCES organization_roles (role) ON UPDATE CASCADE ON DELETE RESTRICT,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (organization_id, user_id)
);

CREATE INDEX organization_members_user_id_idx ON organization_members (user_id);

CREATE TRIGGER organization_members_set_updated_at
BEFORE UPDATE ON organization_members FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

CREATE TABLE organization_invites (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations ON DELETE CASCADE,
    role text NOT NULL REFERENCES organization_roles (role) ON UPDATE CASCADE ON DELETE RESTRICT,
    -- NULL when staff made it.
    invited_by uuid REFERENCES users ON DELETE SET NULL,

    token_hash bytea NOT NULL UNIQUE,
    expires_at timestamptz NOT NULL,
    accepted_at timestamptz,
    accepted_by uuid REFERENCES users ON DELETE SET NULL,
    revoked_at timestamptz,

    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),

    CONSTRAINT organization_invites_expiry_after_created_at CHECK (expires_at > created_at),
    CONSTRAINT organization_invites_accept_consistency CHECK ((accepted_at IS NULL
        AND accepted_by IS NULL) OR (accepted_at IS NOT NULL AND accepted_by IS NOT NULL))
);

CREATE INDEX organization_invites_organization_id_idx ON organization_invites (organization_id);

CREATE TRIGGER organization_invites_set_updated_at
BEFORE UPDATE ON organization_invites FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- A client workspace. The organization can't be deleted while it has any (delete
-- the workspaces first); they keep `plan_code = 'free'`, which nothing reads for
-- them (see `workspace_plans`).
ALTER TABLE workspaces
ADD COLUMN organization_id uuid REFERENCES organizations ON DELETE RESTRICT,
-- Most switchers this client may have; NULL = only the organization's pool limits it.
ADD COLUMN switcher_allocation integer CHECK (switcher_allocation >= 0);

CREATE INDEX workspaces_organization_id_idx ON workspaces (organization_id)
WHERE organization_id IS NOT NULL;

-- What each workspace is entitled to: its plan, or for a client workspace the
-- organization's limits (switchers: the client's allocation, at most the pool).
-- Everything that used to join `plans` through `workspaces.plan_code` joins this
-- as `plans` instead, so the limits come from one place.
CREATE VIEW workspace_plans AS
SELECT
    workspaces.id AS workspace_id,
    plans.id,
    CASE WHEN organizations.id IS NULL THEN plans.code ELSE 'hosted' END AS code,
    COALESCE(organizations.name, plans.name) AS name,
    CASE WHEN organizations.id IS NULL THEN plans.description ELSE '' END AS description,
    plans.is_active,
    plans.position,
    plans.stripe_product_id,
    CASE WHEN organizations.id IS NULL THEN plans.max_switchers
         ELSE LEAST(COALESCE(workspaces.switcher_allocation, organizations.max_switchers),
                    organizations.max_switchers)
    END AS max_switchers,
    COALESCE(organizations.max_stream_servers, plans.max_stream_servers) AS max_stream_servers,
    COALESCE(organizations.max_workspace_members, plans.max_workspace_members)
        AS max_workspace_members,
    COALESCE(organizations.history_days, plans.history_days) AS history_days,
    COALESCE(organizations.custom_chat_bot, plans.custom_chat_bot) AS custom_chat_bot,
    -- Suspended by staff, itself or through its organization.
    COALESCE(workspaces.suspended_at, organizations.suspended_at) AS suspended_at
FROM workspaces
JOIN plans ON plans.code = workspaces.plan_code
LEFT JOIN organizations ON organizations.id = workspaces.organization_id;
