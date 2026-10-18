-- Markers: moments a streamer (or a moderator, from chat) flagged during a
-- stream. Append-only, so only `created_at`; `at` is the moment they mark.
-- They go with their stream (history retention, deleting it).
CREATE TABLE stream_markers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    stream_id UUID NOT NULL REFERENCES streams ON DELETE CASCADE,
    at TIMESTAMPTZ NOT NULL,
    description TEXT,
    created_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX stream_markers_stream_id_at_idx ON stream_markers (stream_id, at);

-- `!marker <description>`; moderators and up, like the other chat controls.
INSERT INTO noalbs_chat_commands (command, default_role, description)
VALUES ('marker', 'MODERATOR', 'Add a marker to the current stream');
