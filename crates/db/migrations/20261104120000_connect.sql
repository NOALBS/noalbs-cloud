-- Devices paired with a switcher through NOALBS Cloud Connect. Only the token
-- hash is stored; pairing codes are short-lived and removed when claimed.
CREATE TABLE connect_devices (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    switcher_id uuid NOT NULL REFERENCES switchers ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    os text NOT NULL CHECK (os IN ('windows', 'linux', 'macos')),
    app_version text NOT NULL CHECK (char_length(app_version) <= 50),
    token_hash text NOT NULL UNIQUE,
    created_by uuid REFERENCES users ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz,
    revoked_at timestamptz
);

CREATE INDEX connect_devices_switcher_id ON connect_devices (switcher_id);

CREATE TABLE connect_pairings (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    device_code_hash text NOT NULL UNIQUE,
    user_code text NOT NULL UNIQUE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    os text NOT NULL CHECK (os IN ('windows', 'linux', 'macos')),
    app_version text NOT NULL CHECK (char_length(app_version) <= 50),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    switcher_id uuid REFERENCES switchers ON DELETE CASCADE,
    confirmed_by uuid REFERENCES users ON DELETE SET NULL,
    confirmed_by_name text
);

ALTER TABLE switchers
    ADD COLUMN connect_obs_control text NOT NULL DEFAULT 'admins'
        CHECK (connect_obs_control IN ('owners', 'admins', 'everyone')),
    ADD COLUMN connect_logs text NOT NULL DEFAULT 'admins'
        CHECK (connect_logs IN ('owners', 'admins', 'everyone')),
    ADD COLUMN connect_watchdog text NOT NULL DEFAULT 'restart'
        CHECK (connect_watchdog IN ('off', 'restart', 'restart_resume'));
