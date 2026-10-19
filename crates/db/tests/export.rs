//! "Download my data" (GDPR): what the export holds, and guards that fail when
//! the schema grows without deciding whether the new data is exported.

use db::{export, switcher_events, switchers, users, workspaces, workspaces::WorkspaceRoleEnum};
use futures_util::TryStreamExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Tables `db::export` covers (`account`, or `history` for stream history).
const EXPORTED: &[&str] = &[
    "chat_bots",
    "chat_channels",
    "chat_ignored_chatters",
    "noalbs_chat_command_aliases",
    "noalbs_chat_command_overrides",
    "notices",
    "oauth2_connections",
    "organization_audit",
    "organization_invites",
    "organization_members",
    "organizations",
    "staff_actions",
    "stream_markers",
    "stream_servers",
    "stream_sources",
    "stream_stats",
    "streams",
    "subscription_holds",
    "subscription_items",
    "subscriptions",
    "support_access_log",
    "support_grants",
    "switcher_chat_channels",
    "switcher_events",
    "switchers",
    "user_sessions",
    "users",
    "workspace_invites",
    "workspace_members",
    "workspaces",
];

/// Tables without personal data, and why.
const NOT_PERSONAL: &[(&str, &str)] = &[
    ("addons", "the add-on catalogue"),
    ("chat_platform_state", "the chat service's own settings"),
    (
        "chat_subscriptions",
        "the chat service's record of platform subscriptions for linked channels, which are exported",
    ),
    ("noalbs_chat_commands", "the command catalogue"),
    ("organization_role_permissions", "what each role may do"),
    ("organization_roles", "the role catalogue"),
    ("permissions", "the permission catalogue"),
    ("plans", "the plan catalogue"),
    ("prices", "the price list"),
    ("workspace_role_permissions", "what each role may do"),
    ("workspace_roles", "the role catalogue"),
];

/// Every column that refers to a user, each exported.
const USER_REFERENCES: &[&str] = &[
    "chat_bots.connected_by",
    "chat_channels.authorized_by",
    "notices.created_by",
    "oauth2_connections.user_id",
    "organization_audit.actor_user_id",
    "organization_invites.accepted_by",
    "organization_invites.invited_by",
    "organization_members.user_id",
    "organizations.created_by",
    "staff_actions.staff_user_id",
    "staff_actions.target_user_id",
    "support_access_log.staff_user_id",
    "support_grants.granted_by",
    "support_grants.requested_by",
    "switcher_chat_channels.linked_by",
    "workspace_invites.accepted_by",
    "workspace_invites.invited_by",
    "workspace_members.user_id",
    "workspaces.created_by",
];

/// Stored in every secret column of the fixture; must never be exported.
const SECRET: &str = "s3cret-that-must-not-leak";

fn sorted(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut items: Vec<String> = items.into_iter().collect();
    items.sort();
    items
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn every_table_is_exported_or_not_personal(pool: PgPool) {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
           AND table_name NOT LIKE '\\_sqlx%'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let known = sorted(
        EXPORTED
            .iter()
            .chain(NOT_PERSONAL.iter().map(|(table, _)| table))
            .map(|table| (*table).to_owned()),
    );
    assert_eq!(
        sorted(tables),
        known,
        "A table was added or dropped: export it in db::export (and add it to EXPORTED) \
         or add it to NOT_PERSONAL with a reason"
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn every_reference_to_a_user_is_exported(pool: PgPool) {
    let references: Vec<String> = sqlx::query_scalar(
        "SELECT constraints.conrelid::regclass::text || '.' || columns.attname
         FROM pg_constraint AS constraints
         JOIN pg_attribute AS columns
             ON columns.attrelid = constraints.conrelid
            AND columns.attnum = ANY (constraints.conkey)
         WHERE constraints.contype = 'f' AND constraints.confrelid = 'users'::regclass",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        sorted(references),
        sorted(USER_REFERENCES.iter().map(|column| (*column).to_owned())),
        "A column referring to users was added or dropped: export what it records about \
         the user in db::export::account and list it in USER_REFERENCES"
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn every_binary_switcher_column_is_a_secret(pool: PgPool) {
    // Switcher settings are exported whole, so a new encrypted or hashed column
    // would be exported unless it's left out.
    let binary: Vec<String> = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = 'switchers' AND data_type = 'bytea'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    for column in binary {
        assert!(
            export::SWITCHER_SECRETS.contains(&column.as_str()),
            "switchers.{column} is binary: add it to db::export::SWITCHER_SECRETS"
        );
    }
}

struct Fixture {
    kate: Uuid,
    sam: Uuid,
    /// Kate's own switcher, in the workspace she owns.
    backpack: Uuid,
    /// In Sam's workspace, where Kate is an admin.
    sams: Uuid,
}

/// Kate: a workspace of her own with a switcher, stream server, chat channel,
/// billing and history; admin in Sam's workspace and in a client workspace of
/// the organization she's on; and records about her elsewhere.
async fn fixture(pool: &PgPool) -> Fixture {
    let kate = users::create(pool).await.unwrap().id;
    let sam = users::create(pool).await.unwrap().id;
    let secret = SECRET.as_bytes();

    let connection: Uuid = sqlx::query_scalar(
        "INSERT INTO oauth2_connections
             (user_id, provider, access_token, refresh_token, provider_id, provider_username, scopes)
         VALUES ($1, 'TWITCH', $2, $2, '1001', 'kate', '{user:read:chat}')
         RETURNING id",
    )
    .bind(kate)
    .bind(SECRET)
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO user_sessions (oauth2_connection_id, user_agent, expires_at)
         VALUES ($1, 'Firefox', now() + interval '30 days')",
    )
    .bind(connection)
    .execute(pool)
    .await
    .unwrap();

    // Her own workspace.
    let own = workspaces::create(pool, kate, "Kate", "kate")
        .await
        .unwrap();
    let backpack = switchers::create(pool, own.id, "Backpack", secret)
        .await
        .unwrap()
        .id;
    sqlx::query(
        "UPDATE switchers
         SET password = $2, belabox_remote_key = $2, moblin_password = $2, trigger_low = 1234
         WHERE id = $1",
    )
    .bind(backpack)
    .bind(secret)
    .execute(pool)
    .await
    .unwrap();
    let server: Uuid = sqlx::query_scalar(
        "INSERT INTO stream_servers (switcher_id, name, kind, stats_url, details)
         VALUES ($1, 'Home', 'SLS', 'http://sls.example/stats',
                 jsonb_build_object('publisher', 'live/kate',
                                    'encrypted_password', $2::text, 'encrypted_api_key', $2::text))
         RETURNING id",
    )
    .bind(backpack)
    .bind(SECRET)
    .fetch_one(pool)
    .await
    .unwrap();
    let channel: Uuid = sqlx::query_scalar(
        "INSERT INTO chat_channels
             (platform, platform_channel_id, login, display_name, access_token, refresh_token,
              authorized_by)
         VALUES ('KICK', '200', 'kate', 'Kate', $1, $1, $2)
         RETURNING id",
    )
    .bind(secret)
    .bind(kate)
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO switcher_chat_channels (switcher_id, chat_channel_id, linked_by)
         VALUES ($1, $2, $3)",
    )
    .bind(backpack)
    .bind(channel)
    .bind(kate)
    .execute(pool)
    .await
    .unwrap();
    for sql in [
        "INSERT INTO chat_ignored_chatters (switcher_id, platform, platform_user_id, platform_username)
         VALUES ($1, 'KICK', '77', 'bob')",
        "INSERT INTO noalbs_chat_command_overrides (switcher_id, command, enabled)
         VALUES ($1, 'fix', false)",
    ] {
        sqlx::query(sql)
            .bind(backpack)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO workspace_invites (workspace_id, role, invited_by, token_hash, expires_at)
         VALUES ($1, 'operator', $2, $3, now() + interval '1 day')",
    )
    .bind(own.id)
    .bind(kate)
    .bind(secret)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO support_grants (workspace_id, level, granted_by, expires_at)
         VALUES ($1, 'view', $2, now() + interval '1 day')",
    )
    .bind(own.id)
    .bind(kate)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE workspaces SET stripe_customer_id = 'cus_1' WHERE id = $1")
        .bind(own.id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "WITH subscription AS (
             INSERT INTO subscriptions (workspace_id, stripe_subscription_id, billing_interval, status)
             VALUES ($1, 'sub_1', 'month', 'active')
             RETURNING id
         )
         INSERT INTO subscription_items (subscription_id, stripe_item_id, stripe_price_id, kind, code, quantity)
         SELECT id, 'si_1', 'price_1', 'plan', 'pro', 1 FROM subscription",
    )
    .bind(own.id)
    .execute(pool)
    .await
    .unwrap();
    add_stream(pool, backpack, server).await;

    // Sam's workspace, where Kate is an admin: listed, but its switchers are Sam's.
    let crew = workspaces::create(pool, sam, "Crew", "crew").await.unwrap();
    workspaces::add_member(pool, crew.id, kate, WorkspaceRoleEnum::Admin)
        .await
        .unwrap();
    let sams = switchers::create(pool, crew.id, "Sam's", b"sams")
        .await
        .unwrap()
        .id;
    let sams_server: Uuid = sqlx::query_scalar(
        "INSERT INTO stream_servers (switcher_id, name, kind, stats_url)
         VALUES ($1, 'Sam', 'SLS', 'http://sam.example/stats') RETURNING id",
    )
    .bind(sams)
    .fetch_one(pool)
    .await
    .unwrap();
    add_stream(pool, sams, sams_server).await;

    // An organization Kate is on, which manages a client workspace for her.
    let organization: Uuid = sqlx::query_scalar(
        "INSERT INTO organizations
             (name, url_slug, max_switchers, max_stream_servers, max_workspace_members, history_days)
         VALUES ('Nomad', 'nomad', 10, 5, 5, 30) RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO organization_members (organization_id, user_id, role) VALUES ($1, $2, 'admin')",
    )
    .bind(organization)
    .bind(kate)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO organization_audit (organization_id, actor_user_id, actor_name, actor_kind, action, subject)
         VALUES ($1, $2, 'kate', 'team', 'client_created', 'Client')",
    )
    .bind(organization)
    .bind(kate)
    .execute(pool)
    .await
    .unwrap();
    let client: Uuid = sqlx::query_scalar(
        "INSERT INTO workspaces (name, url_slug, organization_id) VALUES ('Client', 'client', $1)
         RETURNING id",
    )
    .bind(organization)
    .fetch_one(pool)
    .await
    .unwrap();
    workspaces::add_member(pool, client, kate, WorkspaceRoleEnum::Admin)
        .await
        .unwrap();
    switchers::create(pool, client, "Van", b"van")
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO staff_actions (staff_name, action, target_user_id, reason)
         VALUES ('mod', 'sign_out_user', $1, 'compromised')",
    )
    .bind(kate)
    .execute(pool)
    .await
    .unwrap();

    Fixture {
        kate,
        sam,
        backpack,
        sams,
    }
}

/// A stream on `switcher` with a source, a stats bucket, a marker and an event.
async fn add_stream(pool: &PgPool, switcher: Uuid, server: Uuid) {
    let stream: Uuid = sqlx::query_scalar(
        "INSERT INTO streams (switcher_id, started_at) VALUES ($1, now()) RETURNING id",
    )
    .bind(switcher)
    .fetch_one(pool)
    .await
    .unwrap();
    for sql in [
        "INSERT INTO stream_sources (stream_id, server_id, name, kind) VALUES ($1, $2, 'Home', 'SLS')",
        "INSERT INTO stream_stats
             (stream_id, server_id, bucket_at, samples, bitrate_sum, bitrate_min, bitrate_max, raw)
         VALUES ($1, $2, now(), 10, 50000, 4000, 6000, '{\"publisher\": \"live/kate\"}')",
    ] {
        sqlx::query(sql)
            .bind(stream)
            .bind(server)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO stream_markers (stream_id, at, description, created_by)
         VALUES ($1, now(), 'crash', 'kate (Web)')",
    )
    .bind(stream)
    .execute(pool)
    .await
    .unwrap();
    switcher_events::insert(
        pool,
        switcher,
        "scene_switched",
        json!({ "by": "kate (Web)" }),
    )
    .await
    .unwrap();
}

fn workspace<'a>(export: &'a Value, name: &str) -> &'a Value {
    export["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|workspace| workspace["name"] == name)
        .unwrap_or_else(|| panic!("workspace {name} is exported"))
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn the_export_holds_what_is_kept_about_the_user(pool: PgPool) {
    let fixture = fixture(&pool).await;
    let export = export::account(&pool, fixture.kate).await.unwrap().unwrap();
    let export = serde_json::to_value(&export).unwrap();

    assert_eq!(export["account"]["id"], json!(fixture.kate));
    assert_eq!(export["connections"][0]["provider_id"], "1001");
    assert_eq!(
        export["connections"][0]["scopes"],
        json!(["user:read:chat"])
    );
    assert_eq!(export["sessions"][0]["user_agent"], "Firefox");

    let own = workspace(&export, "Kate");
    assert_eq!(own["role"], "owner");
    assert_eq!(own["created_by_you"], true);
    assert_eq!(own["billing"]["stripe_customer_id"], "cus_1");
    assert_eq!(
        own["billing"]["subscriptions"][0]["items"][0]["code"],
        "pro"
    );
    let backpack = &own["switchers"][0];
    assert_eq!(backpack["id"], json!(fixture.backpack));
    assert_eq!(backpack["settings"]["trigger_low"], 1234);
    assert_eq!(backpack["has_obs_password"], true);
    assert_eq!(
        backpack["stream_servers"][0]["stats_url"],
        "http://sls.example/stats"
    );
    assert_eq!(
        backpack["stream_servers"][0]["details"],
        json!({ "publisher": "live/kate" })
    );
    assert_eq!(backpack["chat_channels"][0]["login"], "kate");
    assert_eq!(backpack["ignored_chatters"][0]["username"], "bob");
    assert_eq!(
        backpack["command_overrides"],
        json!([{ "command": "fix", "enabled": false, "min_role": null }])
    );

    // Sam's workspace is listed, but its switchers and billing are Sam's.
    let crew = workspace(&export, "Crew");
    assert_eq!(crew["role"], "admin");
    assert_eq!(crew["switchers"], json!([]));
    assert_eq!(crew["billing"], Value::Null);

    // The organization manages this one for her: its switchers are hers.
    let client = workspace(&export, "Client");
    assert_eq!(client["managed_by"], "Nomad");
    assert_eq!(client["plan"], Value::Null);
    assert_eq!(client["switchers"][0]["name"], "Van");
    assert_eq!(client["billing"], Value::Null);

    assert_eq!(export["organizations"][0]["name"], "Nomad");
    assert_eq!(export["invites"][0]["created_by_you"], true);
    assert_eq!(
        export["chat"]["channels_authorized"][0]["channel_id"],
        "200"
    );
    assert_eq!(export["chat"]["links"][0]["switcher"], "Backpack");
    assert_eq!(export["support_grants"][0]["granted_by_you"], true);
    assert_eq!(export["organization_audit"][0]["action"], "client_created");
    let action = &export["staff_actions"][0];
    assert_eq!(action["about_you"], true);
    assert_eq!(action["reason"], "compromised");
    // Another staff member's name is left out.
    assert_eq!(action["staff_name"], Value::Null);

    assert_no_secrets(&export.to_string());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn stream_history_is_exported_for_the_users_own_switchers(pool: PgPool) {
    let fixture = fixture(&pool).await;
    let lines: Vec<String> = export::history(&pool, fixture.kate)
        .try_collect()
        .await
        .unwrap();
    let lines: Vec<Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();

    let types: Vec<&str> = lines
        .iter()
        .map(|line| line["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["stream", "stats", "event"]);
    assert_eq!(lines[0]["switcher_id"], json!(fixture.backpack));
    assert_eq!(lines[0]["switcher"], "Backpack");
    assert_eq!(lines[0]["sources"][0]["name"], "Home");
    assert_eq!(lines[0]["markers"][0]["description"], "crash");
    assert_eq!(lines[1]["raw"]["publisher"], "live/kate");
    assert_eq!(lines[1]["stream_id"], lines[0]["id"]);
    assert_eq!(lines[2]["switcher_id"], json!(fixture.backpack));
    // Nothing from Sam's switcher.
    for line in &lines {
        assert!(!line.to_string().contains(&fixture.sams.to_string()));
    }

    // Sam gets his, and nothing of Kate's.
    let sams: Vec<String> = export::history(&pool, fixture.sam)
        .try_collect()
        .await
        .unwrap();
    assert_eq!(sams.len(), 3);
    let stream: Value = serde_json::from_str(&sams[0]).unwrap();
    assert_eq!(stream["switcher_id"], json!(fixture.sams));
    assert!(
        sams.iter()
            .all(|line| !line.contains(&fixture.backpack.to_string()))
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn there_is_nothing_to_export_for_an_unknown_user(pool: PgPool) {
    assert!(
        export::account(&pool, Uuid::new_v4())
            .await
            .unwrap()
            .is_none()
    );
}

/// Neither the secret nor its hex encoding (how JSON shows a binary column).
fn assert_no_secrets(text: &str) {
    let hex: String = SECRET.bytes().map(|byte| format!("{byte:02x}")).collect();
    assert!(!text.contains(SECRET), "a secret was exported: {text}");
    assert!(!text.contains(&hex), "a binary secret was exported: {text}");
}
