-- NOALBS's offline timeout: stop the stream after this many minutes on the BRB
-- scene with no feed. NULL = off.
ALTER TABLE switchers
    ADD COLUMN offline_timeout INTEGER CHECK (offline_timeout > 0);
