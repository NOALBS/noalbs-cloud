-- Emergency ("break-glass") support access: staff let themselves in without the
-- workspace's permission, for abuse, a legal obligation or a security problem.
-- Always view-only and short, with a reason; owners and admins see it until
-- they acknowledge it. It never replaces or ends the workspace's own grant.
ALTER TABLE support_grants
    ADD COLUMN break_glass BOOLEAN NOT NULL DEFAULT false,
    -- The staff member who opened it (break-glass only).
    ADD COLUMN requested_by UUID REFERENCES users ON DELETE SET NULL,
    ADD COLUMN reason TEXT,
    ADD COLUMN acknowledged_at TIMESTAMPTZ,
    ADD CONSTRAINT support_grants_break_glass_reason CHECK (NOT break_glass OR reason IS NOT NULL);
