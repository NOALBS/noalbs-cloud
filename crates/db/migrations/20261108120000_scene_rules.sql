-- Server combinations: while every one of a rule's stream servers has a feed,
-- the switcher uses the rule's scenes instead of the in-use server's. Rules are
-- tried in `priority` order; the first match wins. NULL scenes fall back to the
-- in-use server's override, then the switcher's scene.
CREATE TABLE scene_rules (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    switcher_id UUID NOT NULL REFERENCES switchers ON DELETE CASCADE,

    priority INTEGER NOT NULL DEFAULT 0,
    enabled  BOOLEAN NOT NULL DEFAULT true,

    scene_live    UUID,
    scene_low     UUID,
    scene_offline UUID,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX scene_rules_switcher_id_priority_idx ON scene_rules (switcher_id, priority);

CREATE TRIGGER scene_rules_set_updated_at
BEFORE UPDATE ON scene_rules FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- The servers of a rule (two or more). Deleting a server deletes the rules it
-- is in (see `stream_servers::delete`), so a rule never quietly narrows.
CREATE TABLE scene_rule_servers (
    scene_rule_id    UUID NOT NULL REFERENCES scene_rules ON DELETE CASCADE,
    stream_server_id UUID NOT NULL REFERENCES stream_servers ON DELETE CASCADE,
    PRIMARY KEY (scene_rule_id, stream_server_id)
);

CREATE INDEX scene_rule_servers_stream_server_id_idx ON scene_rule_servers (stream_server_id);
