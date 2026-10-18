-- Staff powers above workspaces, and a log of every staff action.

-- A suspended workspace's switchers are paused (like being over the plan's
-- limit) and its chat links go quiet; its members see the reason.
ALTER TABLE workspaces
    ADD COLUMN suspended_at TIMESTAMPTZ,
    ADD COLUMN suspended_reason TEXT;

-- A disabled user can't sign in (their sessions are ended when disabled).
ALTER TABLE users
    ADD COLUMN disabled_at TIMESTAMPTZ,
    ADD COLUMN disabled_reason TEXT;

-- What staff did from Admin (suspensions, sign-outs, plan comps, break-glass, …),
-- for other staff to see. Append-only; kept a year. Support work inside
-- workspaces is in `support_access_log`.
CREATE TABLE staff_actions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    staff_user_id UUID REFERENCES users ON DELETE SET NULL,
    -- As they were called then, so the entry outlives their account.
    staff_name TEXT NOT NULL,
    action TEXT NOT NULL,
    -- The workspace it was about, named as it was then (no FK: the entry
    -- outlives the workspace).
    workspace_id UUID,
    workspace_name TEXT,
    switcher_id UUID,
    switcher_name TEXT,
    -- The user it was about; `NULL` once their account is deleted.
    target_user_id UUID REFERENCES users ON DELETE SET NULL,
    reason TEXT,
    details JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(details) = 'object'),

    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX staff_actions_created_at_idx ON staff_actions (created_at DESC);
