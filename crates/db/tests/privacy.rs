//! Deleting workspaces and accounts, and the retention clean-ups (GDPR).

use db::{switcher_events, switchers, users, workspaces, workspaces::WorkspaceRoleEnum};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn deleting_a_workspace_takes_everything_in_it(pool: PgPool) {
    let owner = users::create(&pool).await.unwrap().id;
    let workspace = workspaces::create(&pool, owner, "Kate", "kate")
        .await
        .unwrap();
    let switcher = switchers::create(&pool, workspace.id, "Backpack", b"hash")
        .await
        .unwrap();
    switcher_events::insert(&pool, switcher.id, "obs_connected", json!({}))
        .await
        .unwrap();

    assert!(workspaces::delete(&pool, workspace.id).await.unwrap());
    assert!(!workspaces::delete(&pool, workspace.id).await.unwrap());
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM switchers WHERE id = $1",
            switcher.id
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM switcher_events WHERE switcher_id = $1",
            switcher.id
        )
        .await,
        0
    );
    // The account stays.
    assert!(users::get_by_id(&pool, owner).await.unwrap().is_some());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn an_account_can_go_while_a_workspace_it_created_stays(pool: PgPool) {
    let creator = users::create(&pool).await.unwrap().id;
    let other = users::create(&pool).await.unwrap().id;
    let shared = workspaces::create(&pool, creator, "Crew", "crew")
        .await
        .unwrap();
    let alone = workspaces::create(&pool, creator, "Solo", "solo")
        .await
        .unwrap();
    workspaces::add_member(&pool, shared.id, other, WorkspaceRoleEnum::Admin)
        .await
        .unwrap();

    // What deleting the account has to deal with: one shared, one alone.
    let owned = workspaces::owned_by(&pool, creator).await.unwrap();
    let summary: Vec<_> = owned
        .iter()
        .map(|workspace| (workspace.url_slug.as_str(), workspace.other_members))
        .collect();
    assert_eq!(summary, [("crew", 1), ("solo", 0)]);
    assert!(workspaces::owned_by(&pool, other).await.unwrap().is_empty());

    // A workspace it created no longer blocks deleting the account.
    workspaces::delete(&pool, alone.id).await.unwrap();
    assert!(users::delete(&pool, creator).await.unwrap());
    let kept = workspaces::get_by_id(&pool, shared.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(kept.created_by, None);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM workspace_members WHERE workspace_id = $1",
            shared.id
        )
        .await,
        1
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn old_activity_and_expired_sessions_are_cleaned_up(pool: PgPool) {
    let owner = users::create(&pool).await.unwrap().id;
    // Free plan: 7 days of history, so activity is kept for the minimum.
    let workspace = workspaces::create(&pool, owner, "Kate", "kate")
        .await
        .unwrap();
    let switcher = switchers::create(&pool, workspace.id, "Backpack", b"hash")
        .await
        .unwrap();
    for days_ago in [1, switcher_events::MIN_RETENTION_DAYS + 5] {
        let event = switcher_events::insert(&pool, switcher.id, "obs_connected", json!({}))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE switcher_events SET created_at = now() - make_interval(days => $2) WHERE id = $1",
        )
        .bind(event.id)
        .bind(days_ago)
        .execute(&pool)
        .await
        .unwrap();
    }
    assert_eq!(switcher_events::delete_expired(&pool).await.unwrap(), 1);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM switcher_events WHERE switcher_id = $1",
            switcher.id
        )
        .await,
        1
    );

    sqlx::query(
        "INSERT INTO oauth2_connections (user_id, provider, access_token, provider_id, provider_username)
         VALUES ($1, 'DEV', '', 'dev-kate', 'kate')",
    )
    .bind(owner)
    .execute(&pool)
    .await
    .unwrap();
    let connection: Uuid =
        sqlx::query_scalar("SELECT id FROM oauth2_connections WHERE user_id = $1")
            .bind(owner)
            .fetch_one(&pool)
            .await
            .unwrap();
    for expires_in in [-1, 1] {
        db::sessions::create(
            &pool,
            connection,
            chrono::Utc::now() + chrono::Duration::days(expires_in),
            Some("Firefox"),
        )
        .await
        .unwrap();
    }
    assert_eq!(db::sessions::delete_expired(&pool).await.unwrap(), 1);
    assert_eq!(
        db::sessions::list_for_user(&pool, owner)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn using_a_session_keeps_it_going(pool: PgPool) {
    let user = users::create(&pool).await.unwrap().id;
    sqlx::query(
        "INSERT INTO oauth2_connections (user_id, provider, access_token, provider_id, provider_username)
         VALUES ($1, 'DEV', '', 'dev-kate', 'kate')",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    let connection: Uuid =
        sqlx::query_scalar("SELECT id FROM oauth2_connections WHERE user_id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    // A day left, last used 10 minutes ago (inserted as such: the updated_at
    // trigger would overwrite it on an UPDATE).
    let session: Uuid = sqlx::query_scalar(
        "INSERT INTO user_sessions (oauth2_connection_id, expires_at, updated_at)
         VALUES ($1, now() + interval '1 day', now() - interval '10 minutes')
         RETURNING id",
    )
    .bind(connection)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        db::sessions::user_id_for_active_session(&pool, session)
            .await
            .unwrap(),
        Some(user)
    );
    let expires_at: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT expires_at FROM user_sessions WHERE id = $1")
            .bind(session)
            .fetch_one(&pool)
            .await
            .unwrap();
    let left = expires_at - chrono::Utc::now();
    assert!(
        left > db::sessions::IDLE_LIFETIME - chrono::Duration::minutes(1),
        "extended to {left}"
    );
}
