//! Staff powers: suspending workspaces, disabling users, unlinking channels,
//! and the staff audit log.

use chrono::{TimeDelta, Utc};
use db::{
    chat::{self, ChatPlatform, Grant, PlatformAccount},
    sessions, staff_actions, support, switchers, users, workspaces,
};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

fn kick(id: &'static str) -> PlatformAccount<'static> {
    PlatformAccount {
        platform: ChatPlatform::Kick,
        id,
        login: id,
        display_name: id,
    }
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn a_suspended_workspace_is_paused_and_quiet_in_chat(pool: PgPool) {
    let owner = users::create(&pool).await.unwrap().id;
    let workspace = workspaces::create(&pool, owner, "Kate", "kate")
        .await
        .unwrap()
        .id;
    let switcher = switchers::create(&pool, workspace, "Backpack", b"hash")
        .await
        .unwrap()
        .id;
    chat::link_channel(&pool, switcher, kick("kate"), Grant::default(), owner)
        .await
        .unwrap();
    assert!(!switchers::is_paused(&pool, switcher).await.unwrap());
    assert_eq!(
        chat::links_for_channel(&pool, ChatPlatform::Kick, "kate")
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(chat::wanted_subscriptions(&pool).await.unwrap().len(), 1);

    assert!(
        workspaces::set_suspension(&pool, workspace, Some("Abuse report 42"))
            .await
            .unwrap()
    );
    assert!(switchers::is_paused(&pool, switcher).await.unwrap());
    assert!(switchers::list_summaries(&pool, workspace).await.unwrap()[0].paused);
    assert_eq!(
        workspaces::suspension(&pool, workspace)
            .await
            .unwrap()
            .as_deref(),
        Some("Abuse report 42")
    );
    let summary = workspaces::get_summary(&pool, workspace, owner)
        .await
        .unwrap()
        .unwrap();
    assert!(summary.suspended_at.is_some());
    assert!(
        chat::links_for_channel(&pool, ChatPlatform::Kick, "kate")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(chat::wanted_subscriptions(&pool).await.unwrap().is_empty());
    assert!(
        chat::channels_of_switcher(&pool, switcher)
            .await
            .unwrap()
            .is_empty()
    );

    workspaces::set_suspension(&pool, workspace, None)
        .await
        .unwrap();
    assert!(!switchers::is_paused(&pool, switcher).await.unwrap());
    assert_eq!(
        workspaces::suspension(&pool, workspace).await.unwrap(),
        None
    );
    assert_eq!(chat::wanted_subscriptions(&pool).await.unwrap().len(), 1);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn disabling_a_user_ends_their_sessions(pool: PgPool) {
    let user = users::create(&pool).await.unwrap().id;
    let connection_id: Uuid = sqlx::query_scalar(
        "INSERT INTO oauth2_connections (user_id, provider, access_token, provider_id, provider_username)
         VALUES ($1, 'DEV', 'dev', 'mia', 'mia') RETURNING id",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    let session = sessions::create(&pool, connection_id, Utc::now() + TimeDelta::days(1), None)
        .await
        .unwrap();
    assert_eq!(
        sessions::user_id_for_active_session(&pool, session.id)
            .await
            .unwrap(),
        Some(user)
    );
    let loaded = sessions::user_for_active_session(&pool, session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.id, user);
    assert!(!loaded.staff);

    assert!(
        users::set_disabled(&pool, user, Some("Spam"))
            .await
            .unwrap()
    );
    let disabled = users::get_by_id(&pool, user).await.unwrap().unwrap();
    assert!(disabled.disabled_at.is_some());
    assert_eq!(disabled.disabled_reason.as_deref(), Some("Spam"));
    assert_eq!(
        sessions::user_id_for_active_session(&pool, session.id)
            .await
            .unwrap(),
        None
    );

    // A session made after disabling (a sign-in racing it) doesn't work either.
    let late = sessions::create(&pool, connection_id, Utc::now() + TimeDelta::days(1), None)
        .await
        .unwrap();
    assert_eq!(
        sessions::user_id_for_active_session(&pool, late.id)
            .await
            .unwrap(),
        None
    );

    assert!(users::set_disabled(&pool, user, None).await.unwrap());
    assert_eq!(
        sessions::user_id_for_active_session(&pool, late.id)
            .await
            .unwrap(),
        Some(user)
    );
    assert_eq!(sessions::delete_all_for_user(&pool, user).await.unwrap(), 1);
    assert!(
        !users::set_disabled(&pool, Uuid::new_v4(), Some("x"))
            .await
            .unwrap()
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn unlinking_everywhere_reports_every_link(pool: PgPool) {
    let owner = users::create(&pool).await.unwrap().id;
    let a = workspaces::create(&pool, owner, "A", "a").await.unwrap().id;
    let b = workspaces::create(&pool, owner, "B", "b").await.unwrap().id;
    let on_a = switchers::create(&pool, a, "One", b"1").await.unwrap().id;
    let on_b = switchers::create(&pool, b, "Two", b"2").await.unwrap().id;
    let channel = chat::link_channel(&pool, on_a, kick("mia"), Grant::default(), owner)
        .await
        .unwrap();
    chat::link_channel(&pool, on_b, kick("mia"), Grant::default(), owner)
        .await
        .unwrap();

    let of_a = chat::channels_of_workspace(&pool, a).await.unwrap();
    assert_eq!(of_a.len(), 1);
    assert_eq!(
        chat::links_of_channels(&pool, &[channel.id])
            .await
            .unwrap()
            .len(),
        2
    );

    let unlinked = chat::unlink_everywhere(&pool, channel.id).await.unwrap();
    let workspaces_hit: Vec<_> = unlinked
        .iter()
        .map(|link| link.workspace_name.as_str())
        .collect();
    assert_eq!(workspaces_hit, ["A", "B"]);
    assert!(
        chat::channels_of_workspace(&pool, a)
            .await
            .unwrap()
            .is_empty()
    );
    // The channel (and its grant) stays, so it can be linked again.
    assert!(chat::channel(&pool, channel.id).await.unwrap().is_some());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn the_audit_log_merges_admin_actions_and_support_work(pool: PgPool) {
    let owner = users::create(&pool).await.unwrap().id;
    let staff = users::create(&pool).await.unwrap().id;
    let workspace = workspaces::create(&pool, owner, "Kate", "kate")
        .await
        .unwrap()
        .id;

    staff_actions::record(
        &pool,
        &staff_actions::NewAction {
            staff_user_id: staff,
            staff_name: "helper",
            action: "suspend_workspace",
            workspace_id: Some(workspace),
            workspace_name: Some("Kate"),
            reason: Some("Abuse report 42"),
            details: json!({}),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // Mirrored into the workspace's log, which the audit doesn't list twice.
    support::log_access(
        &pool,
        &support::Access {
            workspace_id: workspace,
            grant_id: None,
            staff_user_id: staff,
            staff_name: "helper",
            method: "PUT",
            route: "/admin/workspaces/{workspace_id}/suspension",
            switcher_id: None,
        },
    )
    .await
    .unwrap();
    let grant = support::grant(
        &pool,
        workspace,
        support::SupportLevel::View,
        owner,
        Utc::now() + TimeDelta::hours(1),
    )
    .await
    .unwrap();
    support::log_access(
        &pool,
        &support::Access {
            workspace_id: workspace,
            grant_id: Some(grant.id),
            staff_user_id: staff,
            staff_name: "helper",
            method: "GET",
            route: "/workspaces/{workspace_id}/switchers",
            switcher_id: None,
        },
    )
    .await
    .unwrap();
    staff_actions::record(
        &pool,
        &staff_actions::NewAction {
            staff_user_id: staff,
            staff_name: "helper",
            action: "disable_user",
            target_user_id: Some(owner),
            reason: Some("Spam"),
            details: serde_json::Value::Null,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let all = staff_actions::audit(&pool, None, None, 50).await.unwrap();
    let kinds: Vec<_> = all
        .iter()
        .map(|entry| (entry.kind.as_str(), entry.action.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("admin", "disable_user"),
            ("support", "/workspaces/{workspace_id}/switchers"),
            ("admin", "suspend_workspace"),
        ]
    );
    assert_eq!(all[0].target_user_id, Some(owner));
    assert_eq!(all[1].grant_level, Some(support::SupportLevel::View));
    assert_eq!(all[2].workspace_slug.as_deref(), Some("kate"));

    let support_only = staff_actions::audit(&pool, Some("support"), None, 50)
        .await
        .unwrap();
    assert_eq!(support_only.len(), 1);
    let older = staff_actions::audit(&pool, None, Some(all[0].created_at), 50)
        .await
        .unwrap();
    assert_eq!(older.len(), 2);

    // Deleting the workspace keeps the staff action, named as it was.
    workspaces::delete(&pool, workspace).await.unwrap();
    let after = staff_actions::audit(&pool, Some("admin"), None, 50)
        .await
        .unwrap();
    assert_eq!(after[1].workspace_name.as_deref(), Some("Kate"));
    assert_eq!(after[1].workspace_slug, None);
}
