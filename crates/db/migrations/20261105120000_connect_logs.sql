CREATE TABLE connect_logs (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    switcher_id uuid NOT NULL REFERENCES switchers ON DELETE CASCADE,
    device_id uuid REFERENCES connect_devices ON DELETE SET NULL,
    kind text NOT NULL CHECK (kind IN ('obs_log', 'crash_report')),
    file_name text NOT NULL,
    file_modified_at timestamptz,
    uploaded_at timestamptz NOT NULL DEFAULT now(),
    size_bytes integer NOT NULL,
    truncated boolean NOT NULL DEFAULT false,
    content bytea NOT NULL
);

CREATE INDEX connect_logs_switcher_uploaded_at
    ON connect_logs (switcher_id, uploaded_at DESC);
