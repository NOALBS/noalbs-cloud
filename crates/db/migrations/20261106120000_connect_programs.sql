ALTER TABLE switchers
    ADD COLUMN connect_programs text NOT NULL DEFAULT 'owners'
        CHECK (connect_programs IN ('owners', 'admins', 'everyone'));
