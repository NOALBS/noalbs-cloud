-- Function to automatically update the 'updated_at' column on row updates.
CREATE FUNCTION set_updated_at () RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$;

-- ---------------------------------------------------------------------------
-- Users and OAuth2 connections
-- ---------------------------------------------------------------------------

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    whitelisted BOOLEAN NOT NULL DEFAULT FALSE,
    staff BOOLEAN NOT NULL DEFAULT FALSE,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TRIGGER users_set_updated_at
BEFORE UPDATE ON users FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- DEV is only used by the API's dev login (`dev_login = true`), never in production.
CREATE TYPE oauth2_provider AS ENUM('TWITCH', 'KICK', 'YOUTUBE', 'DISCORD', 'DEV');

CREATE TABLE oauth2_connections (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users ON DELETE CASCADE,
    provider oauth2_provider NOT NULL,

    access_token TEXT NOT NULL,
    refresh_token TEXT,
    token_type TEXT NOT NULL DEFAULT 'bearer',
    expires_at TIMESTAMPTZ,
    scopes TEXT[] NOT NULL DEFAULT '{}',

    provider_id TEXT NOT NULL,
    provider_username TEXT NOT NULL,
    provider_email TEXT,
    provider_avatar_url TEXT,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (user_id, provider),
    UNIQUE (provider, provider_id)
);

CREATE TRIGGER oauth2_connections_set_updated_at 
BEFORE UPDATE ON oauth2_connections FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- User sessions table to manage user login sessions.
CREATE TABLE user_sessions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    oauth2_connection_id UUID NOT NULL REFERENCES oauth2_connections ON DELETE CASCADE,
    -- The browser that signed in, to tell sessions apart on the account page.
    user_agent TEXT,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    -- We'll use the updated_at column to track the last time the session was
    -- used, and we'll use that to determine if the session is still valid.
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX user_sessions_expires_at_idx ON user_sessions (expires_at);

CREATE TRIGGER user_sessions_set_updated_at
BEFORE UPDATE ON user_sessions FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- ---------------------------------------------------------------------------
-- Permissions and roles for workspaces
-- ---------------------------------------------------------------------------

CREATE TABLE workspace_roles (
    role TEXT PRIMARY KEY,
    rank SMALLINT NOT NULL UNIQUE
);

INSERT INTO
    workspace_roles (role, rank)
VALUES
    ('operator', 0),
    ('admin', 10),
    ('owner', 20);

CREATE TABLE permissions (
    permission  TEXT PRIMARY KEY,
    description TEXT NOT NULL DEFAULT ''
);

INSERT INTO
    permissions (permission, description)
VALUES
    ('account.view', 'Read workspace settings and members'),
    ('account.manage', 'Change workspace settings, invite and remove members'),
    ('switcher.view', 'See switchers, their live status and activity'),
    ('switcher.control', 'Switch scenes, start and stop the stream and recording'),
    ('switcher.manage', 'Create switchers and change their settings and stream servers'),
    ('billing.manage', 'Change the workspace plan and billing details');


CREATE TABLE workspace_role_permissions (
    role TEXT NOT NULL REFERENCES workspace_roles (role) ON UPDATE CASCADE ON DELETE CASCADE,
    permission TEXT NOT NULL REFERENCES permissions (permission) ON UPDATE CASCADE ON DELETE CASCADE,
    PRIMARY KEY (role, permission)
);

INSERT INTO
    workspace_role_permissions (role, permission)
VALUES
    ('operator', 'account.view'),
    ('operator', 'switcher.view'),
    ('operator', 'switcher.control'),
    ('admin', 'account.view'),
    ('admin', 'account.manage'),
    ('admin', 'switcher.view'),
    ('admin', 'switcher.control'),
    ('admin', 'switcher.manage'),
    ('owner', 'account.view'),
    ('owner', 'account.manage'),
    ('owner', 'switcher.view'),
    ('owner', 'switcher.control'),
    ('owner', 'switcher.manage'),
    ('owner', 'billing.manage');

-- ----------------------------------------------------------------------------
-- Workspaces, members, invites
-- ----------------------------------------------------------------------------

CREATE TABLE workspaces (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_by UUID NOT NULL REFERENCES users ON DELETE RESTRICT,

    name TEXT NOT NULL UNIQUE,
    url_slug TEXT NOT NULL UNIQUE,
    stripe_customer_id TEXT UNIQUE,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TRIGGER workspaces_set_updated_at
BEFORE UPDATE ON workspaces FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- The workspace a user last opened, so the dashboard can take them back to it.
ALTER TABLE users
ADD COLUMN last_workspace_id UUID REFERENCES workspaces ON DELETE SET NULL;

CREATE TABLE workspace_members (
    workspace_id UUID NOT NULL REFERENCES workspaces ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users ON DELETE CASCADE,
    role TEXT NOT NULL REFERENCES workspace_roles (role) ON UPDATE CASCADE ON DELETE RESTRICT,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (workspace_id, user_id)
);

CREATE INDEX idx_workspace_members_user_id ON workspace_members (user_id);

CREATE UNIQUE INDEX workspace_members_one_owner_per_workspace
ON workspace_members (workspace_id) WHERE role = 'owner';

CREATE TRIGGER workspace_members_set_updated_at
BEFORE UPDATE ON workspace_members FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

CREATE TABLE workspace_invites (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces ON DELETE CASCADE, 
    role TEXT NOT NULL REFERENCES workspace_roles (role) ON UPDATE CASCADE ON DELETE RESTRICT,
    invited_by UUID REFERENCES users ON DELETE SET NULL,

    token_hash BYTEA NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    accepted_at TIMESTAMPTZ,
    accepted_by UUID REFERENCES users ON DELETE SET NULL,
    revoked_at TIMESTAMPTZ,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT workspace_invites_expiry_after_created_at CHECK (expires_at > created_at),
    CONSTRAINT workspace_invites_accept_consistency CHECK ((accepted_at IS NULL
        AND accepted_by IS NULL) OR (accepted_at IS NOT NULL AND accepted_by IS NOT NULL))
);

CREATE INDEX workspace_invites_workspace_id_idx ON workspace_invites (workspace_id);

CREATE TRIGGER workspace_invites_set_updated_at
BEFORE UPDATE ON workspace_invites FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- ---------------------------------------------------------------------------
-- Plans and subscriptions
-- ---------------------------------------------------------------------------

CREATE TABLE plans (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    code TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    -- Offered to workspaces (inactive plans keep the workspaces already on them).
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    -- Display order, lowest first.
    position INTEGER NOT NULL DEFAULT 0,
    -- Created by the admin section when billing is on; prices are in `prices`.
    stripe_product_id TEXT UNIQUE,

    max_switchers INTEGER NOT NULL CHECK (max_switchers >= 0),
    max_workspace_members INTEGER NOT NULL CHECK (max_workspace_members >= 0),
    -- How many days of stream history are kept.
    history_days INTEGER NOT NULL CHECK (history_days >= 0),
    -- Workspaces may connect their own chat bot account instead of the shared one.
    custom_chat_bot BOOLEAN NOT NULL DEFAULT false,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TRIGGER plans_set_updated_at
BEFORE UPDATE ON plans FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

INSERT INTO
    plans (
        code,
        name,
        position,
        max_switchers,
        max_workspace_members,
        history_days,
        custom_chat_bot
    )
VALUES
    ('free', 'Free', 0, 1, 3, 7, false),
    ('pro', 'Pro', 1, 5, 10, 90, true);

-- The plan a workspace is entitled to: the plan item of its live subscription,
-- kept in sync by the Stripe webhook (`free` without one). Staff can also set it
-- by hand for workspaces without a subscription.
ALTER TABLE workspaces
ADD COLUMN plan_code TEXT NOT NULL DEFAULT 'free' REFERENCES plans (code) ON UPDATE CASCADE ON DELETE RESTRICT;

-- A workspace's Stripe subscriptions, mirrored by the webhook. What it pays for
-- is in `subscription_items`: the base plan and any add-ons.
CREATE TABLE subscriptions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid (),
    workspace_id UUID NOT NULL REFERENCES workspaces ON DELETE CASCADE,

    stripe_subscription_id TEXT NOT NULL UNIQUE,
    -- All items of a subscription share one interval.
    billing_interval TEXT NOT NULL CHECK (billing_interval IN ('month', 'year')),
    status TEXT NOT NULL CHECK (status IN (
            'incomplete', 'incomplete_expired', 'trialing',
            'active', 'past_due', 'canceled', 'unpaid', 'paused'
        )),

    current_period_start TIMESTAMPTZ,
    current_period_end TIMESTAMPTZ,
    cancel_at_period_end BOOLEAN NOT NULL DEFAULT FALSE,
    canceled_at TIMESTAMPTZ,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX subscriptions_workspace_id_idx ON subscriptions (workspace_id);

-- A workspace can have a history of canceled subscriptions but only one that
-- currently entitles it to anything.
CREATE UNIQUE INDEX subscriptions_one_live_per_workspace_idx
    ON subscriptions (workspace_id)
    WHERE status IN ('trialing', 'active', 'past_due');

CREATE TRIGGER subscriptions_set_updated_at
BEFORE UPDATE ON subscriptions FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- What a subscription pays for: at most one `plan` item (its `code` is a plan
-- code) and any `addon` items (e.g. a future ingest add-on), each with a
-- quantity. Replaced whenever the webhook syncs the subscription.
CREATE TABLE subscription_items (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid (),
    subscription_id UUID NOT NULL REFERENCES subscriptions ON DELETE CASCADE,

    stripe_item_id TEXT NOT NULL UNIQUE,
    stripe_price_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('plan', 'addon')),
    code TEXT NOT NULL,
    quantity INTEGER NOT NULL CHECK (quantity > 0),

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX subscription_items_subscription_id_idx ON subscription_items (subscription_id);
CREATE UNIQUE INDEX subscription_items_one_plan_idx
    ON subscription_items (subscription_id)
    WHERE kind = 'plan';

CREATE TRIGGER subscription_items_set_updated_at
BEFORE UPDATE ON subscription_items FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- What was paid for until the end of a period after it was lowered or removed:
-- Stripe stops billing it on the next invoice, and this keeps the workspace
-- entitled until then. `kind`/`code` as in `subscription_items`; for a plan the
-- quantity is 1.
CREATE TABLE subscription_holds (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid (),
    subscription_id UUID NOT NULL REFERENCES subscriptions ON DELETE CASCADE,

    kind TEXT NOT NULL CHECK (kind IN ('plan', 'addon')),
    code TEXT NOT NULL,
    quantity INTEGER NOT NULL CHECK (quantity > 0),
    until TIMESTAMPTZ NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (subscription_id, kind, code)
);

CREATE INDEX subscription_holds_until_idx ON subscription_holds (until);

CREATE TRIGGER subscription_holds_set_updated_at
BEFORE UPDATE ON subscription_holds FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- Add-ons a workspace can buy on top of (or without) a plan, per unit. Prices
-- are in `prices`. `status`: `hidden` (not listed, except to workspaces that
-- already have it), `coming_soon` (listed, not for sale yet), `unavailable`
-- (listed, sold before but paused, e.g. at capacity) or `available` (for sale
-- once it has a price).
CREATE TABLE addons (
    code TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    -- What one unit is called, e.g. "slot".
    unit TEXT NOT NULL,
    max_quantity INTEGER NOT NULL CHECK (max_quantity > 0),
    status TEXT NOT NULL DEFAULT 'hidden'
        CHECK (status IN ('hidden', 'coming_soon', 'unavailable', 'available')),
    position INTEGER NOT NULL DEFAULT 0,
    stripe_product_id TEXT UNIQUE,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TRIGGER addons_set_updated_at
BEFORE UPDATE ON addons FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- Hidden until the ingest server exists.
INSERT INTO addons (code, name, description, unit, max_quantity, status)
VALUES (
    'ingest',
    'Ingest',
    'Send SRT or RTMP from your encoder to a NOALBS ingest server, no server of your own needed.',
    'slot',
    5,
    'hidden'
);

-- What plans and add-ons cost: one Stripe price per product and billing
-- interval, created from the admin section. Changing a price archives the old
-- one (`active = false`); subscriptions on it keep it (grandfathered), and the
-- row still maps it to its plan or add-on. Codes aren't foreign keys because
-- they point at `plans` or `addons` by `kind`. Stripe IDs differ between test
-- and live mode, so each mode needs its own database.
CREATE TABLE prices (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid (),
    kind TEXT NOT NULL CHECK (kind IN ('plan', 'addon')),
    code TEXT NOT NULL,
    billing_interval TEXT NOT NULL CHECK (billing_interval IN ('month', 'year')),
    -- In the currency's smallest unit (cents); per unit for add-ons.
    amount INTEGER NOT NULL CHECK (amount >= 0),
    currency TEXT NOT NULL,
    stripe_price_id TEXT NOT NULL UNIQUE,
    -- Sold to new subscriptions.
    active BOOLEAN NOT NULL DEFAULT TRUE,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX prices_one_active_idx
    ON prices (kind, code, billing_interval)
    WHERE active;

CREATE TRIGGER prices_set_updated_at
BEFORE UPDATE ON prices FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- ---------------------------------------------------------------------------
-- Switcher
-- ---------------------------------------------------------------------------

CREATE TABLE switchers (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id       UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name               TEXT NOT NULL,

    -- --- switcher: scenes ---------------------------------------------------
    -- Driven automatically by the bitrate switcher.
    scene_live    UUID,
    scene_low     UUID,
    scene_offline UUID,

    -- Reached only via chat commands (!starting, !ending, !privacy). NULL means
    -- the streamer has not configured one and the command should decline.
    -- Maybe we'll allow users to create these such command scenes in the future
    scene_starting UUID,
    scene_ending   UUID,
    scene_privacy  UUID,
    scene_refresh  UUID,

    -- --- switcher: triggers -------------------------------------------------
    -- Bitrate in kbps, RTT in ms. NULL turns a trigger off (0 is not allowed,
    -- since an RTT trigger of 0 would always fire).
    trigger_low         INTEGER DEFAULT 800  CHECK (trigger_low > 0),
    trigger_offline     INTEGER              CHECK (trigger_offline > 0),
    trigger_rtt         INTEGER DEFAULT 2500 CHECK (trigger_rtt > 0),
    trigger_rtt_offline INTEGER              CHECK (trigger_rtt_offline > 0),

    -- --- switcher: behaviour ------------------------------------------------
    bitrate_switcher_enabled    BOOLEAN NOT NULL DEFAULT true,
    only_switch_when_streaming  BOOLEAN NOT NULL DEFAULT true,
    instantly_switch_on_recover BOOLEAN NOT NULL DEFAULT true,
    auto_switch_notification    BOOLEAN NOT NULL DEFAULT true,

    -- This field might not be needed in the future.
    retry_attempts INTEGER NOT NULL DEFAULT 5 CHECK (retry_attempts >= 0),

    -- --- Other options ------------------------------------------------------
    auto_stop_on_raid      BOOLEAN NOT NULL DEFAULT false,
    announce_raid          BOOLEAN NOT NULL DEFAULT false,
    record_while_streaming BOOLEAN NOT NULL DEFAULT false,
    switch_to_starting     BOOLEAN NOT NULL DEFAULT false,
    switch_to_live         BOOLEAN NOT NULL DEFAULT false,

    -- --- OBS ----------------------------------------------------------------

    connect_token_hash BYTEA NOT NULL UNIQUE,
    password           BYTEA,
    port               INTEGER NOT NULL DEFAULT 4455 CHECK (port > 0 AND port < 65536),
    last_seen_at       TIMESTAMPTZ,

    -- --- chat ---------------------------------------------------------------
    command_prefix          TEXT NOT NULL DEFAULT '!',
    language                TEXT NOT NULL DEFAULT 'en',
    public_commands_enabled BOOLEAN NOT NULL DEFAULT true,
    mod_commands_enabled    BOOLEAN NOT NULL DEFAULT true,

    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (workspace_id, name)
);

CREATE TRIGGER switchers_set_updated_at
BEFORE UPDATE ON switchers FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- Activity feed: scene switches, stream and recording changes, and who did
-- them. `data` holds the kind-specific details.
CREATE TABLE switcher_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    switcher_id UUID NOT NULL REFERENCES switchers ON DELETE CASCADE,
    kind TEXT NOT NULL,
    data JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(data) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX switcher_events_switcher_id_created_at_idx
ON switcher_events (switcher_id, created_at DESC);

-- TODO: For the chatbot and notifications to the chat we need to figure out how
-- to allow the user to specify which channel to send the message to and what
-- platform e.g. Twitch, Kick, YouTube, Discord.

-- ----------------------------------------------------------------------------
-- Stream servers
-- ----------------------------------------------------------------------------

CREATE TYPE stream_server_kind AS ENUM(
    'NGINX', 'NMS', 'NIMBLE', 'SLS', 'BELABOX', 'MEDIAMTX', 'XIU', 'OPENIRL', 'OPENRTMP', 'RIST', 'IRLHOSTING'
);

CREATE TABLE stream_servers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    switcher_id UUID NOT NULL REFERENCES switchers ON DELETE CASCADE,

    name      TEXT    NOT NULL,
    priority  INTEGER NOT NULL DEFAULT 0,
    enabled   BOOLEAN NOT NULL DEFAULT true,
    stats_url TEXT    NOT NULL DEFAULT '',
    details   jsonb   NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(details) = 'object'),
    kind      stream_server_kind NOT NULL,

    override_scene_live    UUID,
    override_scene_low     UUID,
    override_scene_offline UUID,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (switcher_id, name)
);

CREATE INDEX stream_servers_poll_idx ON stream_servers (switcher_id, priority) WHERE enabled;

CREATE TRIGGER stream_servers_set_updated_at
BEFORE UPDATE ON stream_servers FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- ----------------------------------------------------------------------------
-- Stream history
-- ----------------------------------------------------------------------------

-- One OBS stream, from start to stop, kept for the workspace plan's
-- `history_days`. `ended_at` is NULL while it's going (or until the API's upkeep
-- closes a stream whose browser source went away and never came back).
CREATE TABLE streams (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    switcher_id UUID NOT NULL REFERENCES switchers ON DELETE CASCADE,

    started_at     TIMESTAMPTZ NOT NULL,
    ended_at       TIMESTAMPTZ,
    last_sample_at TIMESTAMPTZ,

    -- Seconds the switcher decided the feed was LIVE / LOW / BRB.
    live_seconds    INTEGER NOT NULL DEFAULT 0,
    low_seconds     INTEGER NOT NULL DEFAULT 0,
    offline_seconds INTEGER NOT NULL DEFAULT 0,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX streams_switcher_id_started_at_idx ON streams (switcher_id, started_at DESC);
CREATE UNIQUE INDEX streams_one_open_per_switcher_idx ON streams (switcher_id) WHERE ended_at IS NULL;

CREATE TRIGGER streams_set_updated_at
BEFORE UPDATE ON streams FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- The stream servers that fed a stream, named as they were then. No foreign
-- key to stream_servers, so history outlives a deleted server.
CREATE TABLE stream_sources (
    stream_id UUID NOT NULL REFERENCES streams ON DELETE CASCADE,
    server_id UUID NOT NULL,
    name TEXT NOT NULL,
    kind stream_server_kind NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (stream_id, server_id)
);

-- One row per server per 10 seconds. Sums and counts rather than averages, so
-- a bucket written twice (a switcher restart mid-bucket) merges. Append-mostly,
-- so only `created_at`.
CREATE TABLE stream_stats (
    stream_id UUID NOT NULL REFERENCES streams ON DELETE CASCADE,
    server_id UUID NOT NULL,
    bucket_at TIMESTAMPTZ NOT NULL,

    samples     INTEGER NOT NULL,
    bitrate_sum BIGINT  NOT NULL,
    bitrate_min INTEGER NOT NULL,
    bitrate_max INTEGER NOT NULL,
    rtt_sum     DOUBLE PRECISION NOT NULL DEFAULT 0,
    rtt_samples INTEGER NOT NULL DEFAULT 0,
    rtt_max     DOUBLE PRECISION,

    -- Seconds this server's own reading was LIVE / LOW / BRB.
    live_seconds    INTEGER NOT NULL DEFAULT 0,
    low_seconds     INTEGER NOT NULL DEFAULT 0,
    offline_seconds INTEGER NOT NULL DEFAULT 0,

    -- The server's last raw stats in the bucket.
    raw JSONB,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (stream_id, server_id, bucket_at)
);

-- ----------------------------------------------------------------------------
-- Chat commands
-- ----------------------------------------------------------------------------

CREATE TYPE chat_command_roles AS ENUM('PUBLIC', 'MODERATOR', 'ADMIN');

CREATE TABLE noalbs_chat_commands (
    command TEXT PRIMARY KEY CHECK (command = lower(btrim(command))),
    default_role chat_command_roles NOT NULL,
    description TEXT NOT NULL DEFAULT ''
);

INSERT INTO noalbs_chat_commands (command, default_role, description)
VALUES
    ('start',        'ADMIN',     'Start streaming in OBS'),
    ('stop',         'ADMIN',     'Stop streaming in OBS'),
    ('record',       'ADMIN',     'Toggle recording in OBS'),
    ('collection',   'ADMIN',     'Change the scene collection and profile'),
    ('alias',        'ADMIN',     'Add or remove an alias for a command'),
    ('switch',       'ADMIN',     'Switch to the given scene (fuzzy match)'),
    ('source',       'ADMIN',     'Toggle an OBS source visibility on the current scene'),
    ('live',         'ADMIN',     'Switch to the live scene'),
    ('privacy',      'ADMIN',     'Switch to the privacy scene'),
    ('starting',     'ADMIN',     'Switch to the starting scene'),
    ('ending',       'ADMIN',     'Switch to the ending scene'),
    ('streamserver', 'ADMIN',     'Toggle a stream server on or off'),
    ('noalbs',       'ADMIN',     'NOALBS settings: prefix, retry, lang, ignore, start/stop, instant'),
    ('public',       'ADMIN',     'Enable/disable public commands'),
    ('mod',          'ADMIN',     'Enable/disable mod commands'),
    ('notify',       'ADMIN',     'Enable/disable chat notifications'),
    ('autostop',     'ADMIN',     'Enable/disable auto stop on host/raid'),
    ('trigger',      'MODERATOR', 'Set the low bitrate threshold'),
    ('otrigger',     'MODERATOR', 'Set the offline bitrate threshold'),
    ('rtrigger',     'MODERATOR', 'Set the RTT-based low threshold'),
    ('ortrigger',    'MODERATOR', 'Set the RTT-based offline threshold'),
    ('sourceinfo',   'MODERATOR', 'Report source details in chat'),
    ('serverinfo',   'MODERATOR', 'Report server details in chat'),
    ('fix',          'MODERATOR', 'Try to fix the stream'),
    ('refresh',      'MODERATOR', 'Try to fix the stream'),
    ('bitrate',      'PUBLIC',    'Report the current bitrate');

CREATE TABLE noalbs_chat_command_overrides (
    switcher_id UUID NOT NULL REFERENCES switchers ON DELETE CASCADE,
    command TEXT NOT NULL REFERENCES noalbs_chat_commands (command) ON UPDATE CASCADE ON DELETE CASCADE,
    enabled BOOLEAN NOT NULL DEFAULT true,
    min_role chat_command_roles,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (switcher_id, command)
);

CREATE INDEX noalbs_chat_command_overrides_command_idx ON noalbs_chat_command_overrides (command);

CREATE TRIGGER noalbs_chat_command_overrides_set_updated_at
BEFORE UPDATE ON noalbs_chat_command_overrides FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

CREATE TABLE noalbs_chat_command_aliases (
    switcher_id UUID NOT NULL REFERENCES switchers ON DELETE CASCADE,
    alias TEXT NOT NULL CHECK (alias = lower(btrim(alias))),
    command TEXT NOT NULL REFERENCES noalbs_chat_commands (command) ON UPDATE CASCADE ON DELETE CASCADE,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (switcher_id, alias)
);

CREATE INDEX noalbs_chat_command_aliases_command_idx ON noalbs_chat_command_aliases (command);

CREATE TRIGGER noalbs_chat_command_aliases_set_updated_at
BEFORE UPDATE ON noalbs_chat_command_aliases FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

CREATE TABLE chat_ignored_chatters (
    switcher_id UUID NOT NULL REFERENCES switchers ON DELETE CASCADE,

    platform          oauth2_provider NOT NULL,
    -- Not sure how this will work with YouTube.
    platform_user_id  TEXT NOT NULL,
    platform_username TEXT NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (switcher_id, platform, platform_user_id)
);
-- ----------------------------------------------------------------------------
-- Chat bot
-- ----------------------------------------------------------------------------
-- Streamers link their Twitch/Kick channels to switchers; the chat service reads
-- those channels' chat and answers as a bot account. Subscriptions belong to a
-- channel (and, on Twitch, the bot reading it), never to a switcher: the chat
-- service's reconciler subscribes what the links need and drops what nothing
-- needs any more, so removing one switcher's link never affects another.

-- A bot account the chat service speaks as: the shared NOALBS bot of a platform
-- (`workspace_id` NULL, connected by staff) or a workspace's own bot, when its
-- plan has `custom_chat_bot`. Twitch keeps the bot's grant itself (the chat
-- service uses an app token), so only Kick bots store tokens, encrypted with the
-- data key.
CREATE TABLE chat_bots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    platform oauth2_provider NOT NULL CHECK (platform IN ('TWITCH', 'KICK')),
    workspace_id UUID REFERENCES workspaces ON DELETE CASCADE,

    platform_user_id TEXT NOT NULL,
    login TEXT NOT NULL,
    display_name TEXT NOT NULL,

    access_token BYTEA,
    refresh_token BYTEA,
    expires_at TIMESTAMPTZ,
    scopes TEXT[] NOT NULL DEFAULT '{}',
    -- `revoked`: the platform refused its grant; someone has to connect it again.
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked')),
    connected_by UUID REFERENCES users ON DELETE SET NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- One shared bot per platform, one own bot per workspace and platform.
    UNIQUE NULLS NOT DISTINCT (workspace_id, platform)
);

CREATE TRIGGER chat_bots_set_updated_at
BEFORE UPDATE ON chat_bots FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- A Twitch/Kick channel whose owner allowed the bot in: on Twitch the
-- `channel:bot` grant (kept by Twitch), on Kick a token for event subscriptions
-- (encrypted). One row per channel, however many switchers use it; linking it
-- again refreshes the grant.
CREATE TABLE chat_channels (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    platform oauth2_provider NOT NULL CHECK (platform IN ('TWITCH', 'KICK')),
    platform_channel_id TEXT NOT NULL,
    login TEXT NOT NULL,
    display_name TEXT NOT NULL,

    access_token BYTEA,
    refresh_token BYTEA,
    expires_at TIMESTAMPTZ,
    scopes TEXT[] NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked')),
    -- Who granted it last.
    authorized_by UUID REFERENCES users ON DELETE SET NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (platform, platform_channel_id)
);

CREATE TRIGGER chat_channels_set_updated_at
BEFORE UPDATE ON chat_channels FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- Which channels' chat controls a switcher. A channel on several switchers runs
-- each command on all of them.
CREATE TABLE switcher_chat_channels (
    switcher_id UUID NOT NULL REFERENCES switchers ON DELETE CASCADE,
    chat_channel_id UUID NOT NULL REFERENCES chat_channels ON DELETE CASCADE,
    linked_by UUID REFERENCES users ON DELETE SET NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (switcher_id, chat_channel_id)
);

CREATE INDEX switcher_chat_channels_channel_idx ON switcher_chat_channels (chat_channel_id);

CREATE TRIGGER switcher_chat_channels_set_updated_at
BEFORE UPDATE ON switcher_chat_channels FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- The event subscriptions the chat service made on the platforms: its record to
-- diff against what the links need. Plain platform IDs rather than foreign keys,
-- so a subscription whose channel or bot is gone can still be found and deleted.
-- `platform_bot_id` is the bot reading the chat on Twitch (part of the
-- subscription's condition), '' where there is none (Kick, raids).
CREATE TABLE chat_subscriptions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    platform oauth2_provider NOT NULL CHECK (platform IN ('TWITCH', 'KICK')),
    event_type TEXT NOT NULL,
    platform_channel_id TEXT NOT NULL,
    platform_bot_id TEXT NOT NULL DEFAULT '',
    platform_subscription_id TEXT NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (platform, event_type, platform_channel_id, platform_bot_id),
    UNIQUE (platform, platform_subscription_id)
);

CREATE TRIGGER chat_subscriptions_set_updated_at
BEFORE UPDATE ON chat_subscriptions FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();

-- Settings the chat service instances share, such as `twitch_conduit_id` (one
-- Twitch conduit per environment; the first instance to insert it wins).
CREATE TABLE chat_platform_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TRIGGER chat_platform_state_set_updated_at
BEFORE UPDATE ON chat_platform_state FOR EACH ROW
EXECUTE FUNCTION set_updated_at ();
