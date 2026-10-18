-- What an organization's team (and its customers, in its clients) changed: the
-- organization's audit log. Append-only. Names are kept as they were then, so an
-- entry outlives the account, client or member it was about. Kept for a year like
-- the staff log (`privacy::upkeep`). What NOALBS staff did to the organization is
-- read from `staff_actions` (its `details` hold the `organization_id`).
CREATE TABLE organization_audit (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id uuid NOT NULL REFERENCES organizations ON DELETE CASCADE,
    -- NULL once their account is deleted.
    actor_user_id uuid REFERENCES users ON DELETE SET NULL,
    actor_name text NOT NULL,
    -- The organization's `team`, or a `customer` acting in their client workspace.
    actor_kind text NOT NULL CHECK (actor_kind IN ('team', 'customer')),
    -- A stable snake_case code, e.g. `client_deleted`.
    action text NOT NULL,
    -- What it was about, e.g. the client's or the member's name.
    subject text,
    details jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(details) = 'object'),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX organization_audit_organization_id_created_at_idx
ON organization_audit (organization_id, created_at DESC);
CREATE INDEX organization_audit_created_at_idx ON organization_audit (created_at);
