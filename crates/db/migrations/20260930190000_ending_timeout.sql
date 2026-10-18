-- Stop the stream after this many minutes on the Ending scene. NULL = off.
ALTER TABLE switchers
    ADD COLUMN ending_timeout INTEGER CHECK (ending_timeout > 0);
