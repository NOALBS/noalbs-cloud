//! Support access grants and the log of what staff did.

use chrono::{TimeDelta, Utc};
use db::support::{self, Access, SupportLevel};
use db::{switchers, users, workspaces};
use sqlx::PgPool;
use uuid::Uuid;

async fn workspace(pool: &PgPool) -> (Uuid, Uuid) {
    let owner = users::create(pool).await.unwrap().id;
    let workspace = workspaces::create(pool, owner, "Kate", "kate")
        .await
        .unwrap();
    (workspace.id, owner)
}

fn access<'a>(workspace_id: Uuid, staff: Uuid, method: &'a str, route: &'a str) -> Access<'a> {
    Access {
        workspace_id,
        grant_id: None,
        staff_user_id: staff,
        staff_name: "helper",
        method,
        route,
        switcher_id: None,
    }
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn a_new_grant_replaces_the_one_in_force(pool: PgPool) {
    let (workspace_id, owner) = workspace(&pool).await;
    assert!(
        support::active_grant(&pool, workspace_id)
            .await
            .unwrap()
            .is_none()
    );

    let in_a_day = Utc::now() + TimeDelta::hours(24);
    let first = support::grant(&pool, workspace_id, SupportLevel::View, owner, in_a_day)
        .await
        .unwrap();
    let second = support::grant(&pool, workspace_id, SupportLevel::Control, owner, in_a_day)
        .await
        .unwrap();

    let active = support::active_grant(&pool, workspace_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.id, second.id);
    assert_eq!(active.level, SupportLevel::Control);
    assert_eq!(active.granted_by, Some(owner));
    let first_revoked: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT revoked_at FROM support_grants WHERE id = $1")
            .bind(first.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(first_revoked.is_some());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn grants_end_when_revoked_or_expired(pool: PgPool) {
    let (workspace_id, owner) = workspace(&pool).await;

    support::grant(
        &pool,
        workspace_id,
        SupportLevel::View,
        owner,
        Utc::now() + TimeDelta::hours(1),
    )
    .await
    .unwrap();
    assert!(support::revoke(&pool, workspace_id).await.unwrap());
    assert!(!support::revoke(&pool, workspace_id).await.unwrap());
    assert!(
        support::active_grant(&pool, workspace_id)
            .await
            .unwrap()
            .is_none()
    );

    support::grant(
        &pool,
        workspace_id,
        SupportLevel::View,
        owner,
        Utc::now() - TimeDelta::seconds(1),
    )
    .await
    .unwrap();
    assert!(
        support::active_grant(&pool, workspace_id)
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn reads_are_logged_once_an_hour_and_changes_always(pool: PgPool) {
    let (workspace_id, owner) = workspace(&pool).await;
    let staff = users::create(&pool).await.unwrap().id;
    let switcher = switchers::create(&pool, workspace_id, "Backpack", b"hash")
        .await
        .unwrap();
    let live = "/workspaces/{workspace_id}/switchers/{switcher_id}/live";
    let scene = "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scene";

    for _ in 0..3 {
        support::log_access(
            &pool,
            &Access {
                switcher_id: Some(switcher.id),
                ..access(workspace_id, staff, "GET", live)
            },
        )
        .await
        .unwrap();
    }
    for _ in 0..2 {
        support::log_access(
            &pool,
            &Access {
                switcher_id: Some(switcher.id),
                ..access(workspace_id, staff, "POST", scene)
            },
        )
        .await
        .unwrap();
    }
    // Under a grant it's another entry, so the workspace sees when it started.
    let grant = support::grant(
        &pool,
        workspace_id,
        SupportLevel::View,
        owner,
        Utc::now() + TimeDelta::hours(1),
    )
    .await
    .unwrap();
    support::log_access(
        &pool,
        &Access {
            grant_id: Some(grant.id),
            switcher_id: Some(switcher.id),
            ..access(workspace_id, staff, "GET", live)
        },
    )
    .await
    .unwrap();

    // Something the grant doesn't cover, while it's in force.
    support::log_access(
        &pool,
        &access(
            workspace_id,
            staff,
            "POST",
            "/workspaces/{workspace_id}/billing/checkout",
        ),
    )
    .await
    .unwrap();

    let log = support::log(&pool, workspace_id, None, 50).await.unwrap();
    let methods: Vec<_> = log.iter().map(|entry| entry.method.as_str()).collect();
    assert_eq!(methods, ["POST", "GET", "POST", "POST", "GET"]);
    assert_eq!(log[0].grant_level, None);
    assert_eq!(log[0].grant_in_force, Some(SupportLevel::View));
    assert_eq!(log[1].grant_level, Some(SupportLevel::View));
    assert_eq!(log[1].switcher_name.as_deref(), Some("Backpack"));
    assert_eq!(log[2].grant_level, None);
    assert_eq!(log[2].grant_in_force, None);

    let older = support::log(&pool, workspace_id, Some(log[2].created_at), 50)
        .await
        .unwrap();
    assert_eq!(older.len(), 2);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn old_records_are_deleted(pool: PgPool) {
    let (workspace_id, owner) = workspace(&pool).await;
    let staff = users::create(&pool).await.unwrap().id;

    support::log_access(&pool, &access(workspace_id, staff, "GET", "/old"))
        .await
        .unwrap();
    support::log_access(&pool, &access(workspace_id, staff, "GET", "/new"))
        .await
        .unwrap();
    sqlx::query("UPDATE support_access_log SET created_at = now() - interval '400 days' WHERE route = '/old'")
        .execute(&pool)
        .await
        .unwrap();
    let old = support::grant(
        &pool,
        workspace_id,
        SupportLevel::View,
        owner,
        Utc::now() - TimeDelta::days(400),
    )
    .await
    .unwrap();
    let current = support::grant(
        &pool,
        workspace_id,
        SupportLevel::View,
        owner,
        Utc::now() + TimeDelta::hours(1),
    )
    .await
    .unwrap();

    assert_eq!(support::delete_expired(&pool).await.unwrap(), 2);
    let routes: Vec<_> = support::log(&pool, workspace_id, None, 50)
        .await
        .unwrap()
        .into_iter()
        .map(|entry| entry.route)
        .collect();
    assert_eq!(routes, ["/new"]);
    let grants: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM support_grants")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(grants, [current.id]);
    assert_ne!(old.id, current.id);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn break_glass_sits_beside_the_workspaces_own_grant(pool: PgPool) {
    let (workspace_id, owner) = workspace(&pool).await;
    let staff = users::create(&pool).await.unwrap().id;
    let in_an_hour = Utc::now() + TimeDelta::hours(1);

    let own = support::grant(
        &pool,
        workspace_id,
        SupportLevel::Control,
        owner,
        in_an_hour,
    )
    .await
    .unwrap();
    let emergency = support::break_glass(
        &pool,
        workspace_id,
        staff,
        SupportLevel::Control,
        "Abuse report 123",
        in_an_hour,
    )
    .await
    .unwrap();
    assert!(emergency.break_glass);
    assert_eq!(emergency.level, SupportLevel::Control);
    assert_eq!(emergency.requested_by, Some(staff));
    assert_eq!(emergency.reason.as_deref(), Some("Abuse report 123"));

    // Own grant first, and it's the one the workspace sees as "its" grant.
    let active: Vec<_> = support::active_grants(&pool, workspace_id)
        .await
        .unwrap()
        .into_iter()
        .map(|grant| grant.id)
        .collect();
    assert_eq!(active, [own.id, emergency.id]);
    assert_eq!(
        support::active_grant(&pool, workspace_id)
            .await
            .unwrap()
            .map(|grant| grant.id),
        Some(own.id)
    );

    // Ending the workspace's permission leaves break-glass running, and a new
    // grant doesn't end it either.
    assert!(support::revoke(&pool, workspace_id).await.unwrap());
    support::grant(&pool, workspace_id, SupportLevel::View, owner, in_an_hour)
        .await
        .unwrap();
    assert!(
        support::active_grants(&pool, workspace_id)
            .await
            .unwrap()
            .iter()
            .any(|grant| grant.id == emergency.id)
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn break_glass_is_listed_until_acknowledged(pool: PgPool) {
    let (workspace_id, _) = workspace(&pool).await;
    let staff = users::create(&pool).await.unwrap().id;

    let emergency = support::break_glass(
        &pool,
        workspace_id,
        staff,
        SupportLevel::View,
        "Legal request",
        Utc::now() + TimeDelta::hours(1),
    )
    .await
    .unwrap();
    support::log_access(
        &pool,
        &Access {
            grant_id: Some(emergency.id),
            ..access(
                workspace_id,
                staff,
                "GET",
                "/workspaces/{workspace_id}/switchers",
            )
        },
    )
    .await
    .unwrap();
    let log = support::log(&pool, workspace_id, None, 50).await.unwrap();
    assert!(log[0].break_glass);
    assert_eq!(log[0].grant_in_force, None);

    let listed = support::break_glass_grants(&pool, workspace_id, 30)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].acknowledged_at.is_none());

    assert_eq!(
        support::acknowledge_break_glass(&pool, workspace_id)
            .await
            .unwrap(),
        1
    );
    // Still listed for a while, but seen.
    let listed = support::break_glass_grants(&pool, workspace_id, 30)
        .await
        .unwrap();
    assert!(listed[0].acknowledged_at.is_some());
    sqlx::query("UPDATE support_grants SET created_at = now() - interval '40 days'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        support::break_glass_grants(&pool, workspace_id, 30)
            .await
            .unwrap()
            .is_empty()
    );
}
