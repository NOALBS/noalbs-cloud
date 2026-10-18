-- Notices: a message staff show to every signed-in user as a banner (maintenance,
-- an outage, a changed policy). Shown from `starts_at` until `ends_at` (never
-- when NULL); ending one early sets `ends_at`.
CREATE TABLE notices (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    message text NOT NULL CHECK (char_length(message) BETWEEN 1 AND 500),
    severity text NOT NULL CHECK (severity IN ('info', 'warning', 'critical')),
    starts_at timestamptz NOT NULL DEFAULT now(),
    ends_at timestamptz,
    -- Users may hide it in their browser. Never for critical ones (the API enforces that).
    dismissible boolean NOT NULL DEFAULT TRUE,
    created_by uuid REFERENCES users ON DELETE SET NULL,
    -- Named as it was when posted.
    created_by_name text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (ends_at IS NULL OR ends_at > starts_at)
);

CREATE INDEX notices_ends_at_idx ON notices (ends_at);

CREATE TRIGGER notices_set_updated_at
BEFORE UPDATE ON notices FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();
