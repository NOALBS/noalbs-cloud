CREATE TABLE switcher_preferences (
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    switcher_id uuid NOT NULL REFERENCES switchers(id) ON DELETE CASCADE,
    preferences jsonb NOT NULL CHECK (jsonb_typeof(preferences) = 'object'),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, switcher_id)
);
