//! Database tests. `#[sqlx::test]` creates a fresh, migrated database per test
//! on the server in `DATABASE_URL`.

use chrono::{Duration, Utc};
use db::{
    invites::{self, AcceptOutcome},
    switchers, users, workspaces,
    workspaces::WorkspaceRoleEnum,
};
use sqlx::PgPool;
use uuid::Uuid;

async fn workspace_with_owner(pool: &PgPool, slug: &str) -> (Uuid, Uuid) {
    let owner = users::create(pool).await.unwrap();
    let workspace = workspaces::create(pool, owner.id, slug, slug)
        .await
        .unwrap();
    (workspace.id, owner.id)
}

async fn invite(pool: &PgPool, workspace_id: Uuid, invited_by: Uuid, token: &str) {
    invites::create(
        pool,
        workspace_id,
        WorkspaceRoleEnum::Operator,
        invited_by,
        token.as_bytes(),
        Utc::now() + Duration::days(7),
    )
    .await
    .unwrap();
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn names_may_repeat_but_slugs_may_not(pool: PgPool) {
    let owner = users::create(&pool).await.unwrap();
    workspaces::create(&pool, owner.id, "IRL", "irl-one")
        .await
        .unwrap();
    workspaces::create(&pool, owner.id, "IRL", "irl-two")
        .await
        .unwrap();
    let taken = workspaces::create(&pool, owner.id, "Other", "irl-one")
        .await
        .unwrap_err();
    assert_eq!(taken.constraint(), Some("workspaces_url_slug_key"));
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn only_owned_workspaces_count_toward_the_limit(pool: PgPool) {
    let user = users::create(&pool).await.unwrap().id;
    // Being a member elsewhere doesn't count.
    let (elsewhere, _) = workspace_with_owner(&pool, "elsewhere").await;
    workspaces::add_member(&pool, elsewhere, user, WorkspaceRoleEnum::Admin)
        .await
        .unwrap();

    for slug in ["own-one", "own-two"] {
        let created = workspaces::create_within_limit(&pool, user, slug, slug, 2)
            .await
            .unwrap();
        assert!(created.is_some());
    }
    let refused = workspaces::create_within_limit(&pool, user, "Three", "own-three", 2)
        .await
        .unwrap();
    assert!(refused.is_none());
    let owned: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces WHERE created_by = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(owned, 2);

    // A slug clash is still an error, not "over the limit".
    let other = users::create(&pool).await.unwrap().id;
    let taken = workspaces::create_within_limit(&pool, other, "Clash", "own-one", 2)
        .await
        .unwrap_err();
    assert_eq!(taken.constraint(), Some("workspaces_url_slug_key"));
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn ownership_moves_to_a_member_and_the_owner_stays_as_admin(pool: PgPool) {
    let (workspace_id, owner) = workspace_with_owner(&pool, "handover").await;
    let admin = users::create(&pool).await.unwrap().id;
    workspaces::add_member(&pool, workspace_id, admin, WorkspaceRoleEnum::Admin)
        .await
        .unwrap();
    let stranger = users::create(&pool).await.unwrap().id;

    // Not a member: nothing changes.
    assert!(
        !workspaces::transfer_ownership(&pool, workspace_id, stranger, true)
            .await
            .unwrap()
    );
    let role = |user| workspaces::member_role(&pool, workspace_id, user);
    assert_eq!(role(owner).await.unwrap(), Some(WorkspaceRoleEnum::Owner));

    assert!(
        workspaces::transfer_ownership(&pool, workspace_id, admin, true)
            .await
            .unwrap()
    );
    assert_eq!(role(admin).await.unwrap(), Some(WorkspaceRoleEnum::Owner));
    assert_eq!(role(owner).await.unwrap(), Some(WorkspaceRoleEnum::Admin));
    let workspace = workspaces::get_by_id(&pool, workspace_id)
        .await
        .unwrap()
        .unwrap();
    assert!(workspace.needs_payment_method);

    // And back, which clears the flag when there was no customer.
    assert!(
        workspaces::transfer_ownership(&pool, workspace_id, owner, false)
            .await
            .unwrap()
    );
    assert_eq!(role(owner).await.unwrap(), Some(WorkspaceRoleEnum::Owner));
    assert_eq!(role(admin).await.unwrap(), Some(WorkspaceRoleEnum::Admin));
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn accepting_an_invite_joins_once(pool: PgPool) {
    let (workspace_id, owner) = workspace_with_owner(&pool, "nomad-live").await;
    invite(&pool, workspace_id, owner, "token-1").await;
    let alice = users::create(&pool).await.unwrap().id;
    let bob = users::create(&pool).await.unwrap().id;

    let outcome = invites::accept(&pool, b"token-1", alice).await.unwrap();
    assert_eq!(
        outcome,
        AcceptOutcome::Joined {
            workspace_id,
            url_slug: "nomad-live".into()
        }
    );
    assert_eq!(
        workspaces::member_role(&pool, workspace_id, alice)
            .await
            .unwrap(),
        Some(WorkspaceRoleEnum::Operator)
    );

    // Accepting again as a member is harmless; someone else can't reuse it.
    assert!(matches!(
        invites::accept(&pool, b"token-1", alice).await.unwrap(),
        AcceptOutcome::AlreadyMember { .. }
    ));
    assert_eq!(
        invites::accept(&pool, b"token-1", bob).await.unwrap(),
        AcceptOutcome::AlreadyUsed
    );
    assert!(
        invites::list_open(&pool, workspace_id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn revoked_expired_and_unknown_invites_are_refused(pool: PgPool) {
    let (workspace_id, owner) = workspace_with_owner(&pool, "nomad-live").await;
    let user = users::create(&pool).await.unwrap().id;

    invite(&pool, workspace_id, owner, "revoked").await;
    let open = invites::list_open(&pool, workspace_id).await.unwrap();
    assert!(
        invites::revoke(&pool, workspace_id, open[0].id)
            .await
            .unwrap()
    );
    assert_eq!(
        invites::accept(&pool, b"revoked", user).await.unwrap(),
        AcceptOutcome::Revoked
    );

    invite(&pool, workspace_id, owner, "expired").await;
    sqlx::query(
        "UPDATE workspace_invites
         SET created_at = now() - interval '2 days', expires_at = now() - interval '1 day'
         WHERE token_hash = $1",
    )
    .bind(b"expired".as_slice())
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        invites::accept(&pool, b"expired", user).await.unwrap(),
        AcceptOutcome::Expired
    );

    assert_eq!(
        invites::accept(&pool, b"nope", user).await.unwrap(),
        AcceptOutcome::NotFound
    );
    assert_eq!(
        workspaces::member_role(&pool, workspace_id, user)
            .await
            .unwrap(),
        None
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn free_workspaces_stop_at_three_members_until_pro(pool: PgPool) {
    let (workspace_id, owner) = workspace_with_owner(&pool, "nomad-live").await;

    for token in ["a", "b", "c"] {
        invite(&pool, workspace_id, owner, token).await;
    }
    for token in ["a", "b"] {
        let user = users::create(&pool).await.unwrap().id;
        assert!(matches!(
            invites::accept(&pool, token.as_bytes(), user)
                .await
                .unwrap(),
            AcceptOutcome::Joined { .. }
        ));
    }

    let fourth = users::create(&pool).await.unwrap().id;
    assert_eq!(
        invites::accept(&pool, b"c", fourth).await.unwrap(),
        AcceptOutcome::MemberLimitReached
    );

    workspaces::set_plan(&pool, workspace_id, "pro")
        .await
        .unwrap();
    assert!(matches!(
        invites::accept(&pool, b"c", fourth).await.unwrap(),
        AcceptOutcome::Joined { .. }
    ));
    assert_eq!(
        workspaces::member_count(&pool, workspace_id).await.unwrap(),
        4
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn summaries_carry_role_and_last_workspace_follows_membership(pool: PgPool) {
    let (workspace_id, owner) = workspace_with_owner(&pool, "nomad-live").await;
    let (_, _) = workspace_with_owner(&pool, "someone-else").await;
    let member = users::create(&pool).await.unwrap().id;
    workspaces::add_member(&pool, workspace_id, member, WorkspaceRoleEnum::Admin)
        .await
        .unwrap();

    let owned = workspaces::list_summaries(&pool, owner).await.unwrap();
    assert_eq!(owned.len(), 1);
    assert_eq!(owned[0].role, Some(WorkspaceRoleEnum::Owner));
    assert_eq!(owned[0].switcher_count, 0);

    // Only your own workspaces are listed (staff find the rest under Admin),
    // but any one can be looked up by its slug, with no role if you aren't in it.
    let mine = workspaces::list_summaries(&pool, member).await.unwrap();
    assert_eq!(
        mine.iter().map(|w| w.url_slug.as_str()).collect::<Vec<_>>(),
        ["nomad-live"]
    );
    let other = workspaces::get_summary_by_slug(&pool, "someone-else", member)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(other.role, None);
    assert!(
        workspaces::get_summary_by_slug(&pool, "nobody", member)
            .await
            .unwrap()
            .is_none()
    );

    assert!(
        users::set_last_workspace(&pool, member, workspace_id)
            .await
            .unwrap()
    );
    assert_eq!(
        users::last_workspace_slug(&pool, member)
            .await
            .unwrap()
            .as_deref(),
        Some("nomad-live")
    );

    // After leaving, the old workspace is no longer the default.
    assert!(
        workspaces::remove_member(&pool, workspace_id, member)
            .await
            .unwrap()
    );
    assert_eq!(
        users::last_workspace_slug(&pool, member).await.unwrap(),
        None
    );
    assert!(
        !users::set_last_workspace(&pool, member, workspace_id)
            .await
            .unwrap()
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn member_access_says_who_may_and_whether_the_workspace_is_suspended(pool: PgPool) {
    use db::workspaces::{MemberAccess, WorkspacePermission};

    let (workspace, owner) = workspace_with_owner(&pool, "crew").await;
    let operator = users::create(&pool).await.unwrap().id;
    workspaces::add_member(&pool, workspace, operator, WorkspaceRoleEnum::Operator)
        .await
        .unwrap();
    let outsider = users::create(&pool).await.unwrap().id;

    let access = |user, permission| {
        let pool = pool.clone();
        async move {
            workspaces::member_access(&pool, workspace, user, permission)
                .await
                .unwrap()
        }
    };
    let allowed = MemberAccess {
        allowed: true,
        suspended_reason: None,
        suspended_by_organization: false,
        organization_allowed: false,
        organization_id: None,
        organization_name: None,
    };
    let refused = MemberAccess {
        allowed: false,
        suspended_reason: None,
        suspended_by_organization: false,
        organization_allowed: false,
        organization_id: None,
        organization_name: None,
    };

    assert_eq!(
        access(owner, WorkspacePermission::SWITCHER_MANAGE).await,
        allowed
    );
    assert_eq!(
        access(operator, WorkspacePermission::SWITCHER_CONTROL).await,
        allowed
    );
    assert_eq!(
        access(operator, WorkspacePermission::SWITCHER_MANAGE).await,
        refused
    );
    assert_eq!(
        access(outsider, WorkspacePermission::SWITCHER_VIEW).await,
        refused
    );
    assert!(
        workspaces::has_permission(&pool, workspace, owner, WorkspacePermission::BILLING_MANAGE)
            .await
            .unwrap()
    );

    workspaces::set_suspension(&pool, workspace, Some("Abuse report 42"))
        .await
        .unwrap();
    let suspended = access(operator, WorkspacePermission::SWITCHER_CONTROL).await;
    assert!(suspended.allowed);
    assert_eq!(
        suspended.suspended_reason.as_deref(),
        Some("Abuse report 42")
    );
    // Not a member: the reason comes along, but nothing is allowed.
    let outside = access(outsider, WorkspacePermission::SWITCHER_VIEW).await;
    assert!(!outside.allowed);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn a_member_of_many_workspaces_lists_them_searched_and_paged(pool: PgPool) {
    use db::workspaces::MemberFilter;

    let manager = users::create(&pool).await.unwrap().id;
    // Workspaces of other people the manager works in: operator in two, admin in one.
    let mut names = Vec::new();
    for (slug, role) in [
        ("alpha-live", WorkspaceRoleEnum::Operator),
        ("bravo-irl", WorkspaceRoleEnum::Admin),
        ("charlie-irl", WorkspaceRoleEnum::Operator),
    ] {
        let owner = users::create(&pool).await.unwrap().id;
        let workspace = workspaces::create(&pool, owner, slug, slug)
            .await
            .unwrap()
            .id;
        invites::create(
            &pool,
            workspace,
            role,
            owner,
            slug.as_bytes(),
            Utc::now() + Duration::days(1),
        )
        .await
        .unwrap();
        invites::accept(&pool, slug.as_bytes(), manager)
            .await
            .unwrap();
        switchers::create(&pool, workspace, &format!("{slug}-main"), slug.as_bytes())
            .await
            .unwrap();
        names.push(slug);
    }
    // One of their own, and one they are not in.
    let own = workspaces::create(&pool, manager, "delta-own", "delta-own")
        .await
        .unwrap()
        .id;
    switchers::create(&pool, own, "own-main", b"own")
        .await
        .unwrap();
    let stranger = users::create(&pool).await.unwrap().id;
    let hidden = workspaces::create(&pool, stranger, "echo-hidden", "echo-hidden")
        .await
        .unwrap()
        .id;
    switchers::create(&pool, hidden, "hidden-main", b"hidden")
        .await
        .unwrap();

    let by_name = MemberFilter {
        by_name: true,
        ..Default::default()
    };
    let page = |filter, limit, offset| {
        let pool = pool.clone();
        async move {
            let (rows, total) =
                workspaces::list_summaries_page(&pool, manager, filter, limit, offset)
                    .await
                    .unwrap();
            (
                rows.into_iter().map(|row| row.url_slug).collect::<Vec<_>>(),
                total,
            )
        }
    };

    // Only the ones they are in, in any role; paged, the total counting all of them.
    let (slugs, total) = page(by_name, 25, 0).await;
    assert_eq!(
        slugs,
        ["alpha-live", "bravo-irl", "charlie-irl", "delta-own"]
    );
    assert_eq!(total, 4);
    let (slugs, total) = page(by_name, 2, 2).await;
    assert_eq!(slugs, ["charlie-irl", "delta-own"]);
    assert_eq!(total, 4);

    // Searched by name or URL, in any case.
    let (slugs, total) = page(
        MemberFilter {
            search: Some("IRL"),
            ..by_name
        },
        25,
        0,
    )
    .await;
    assert_eq!(slugs, ["bravo-irl", "charlie-irl"]);
    assert_eq!(total, 2);

    // By role.
    for (role, expected) in [
        (
            WorkspaceRoleEnum::Operator,
            vec!["alpha-live", "charlie-irl"],
        ),
        (WorkspaceRoleEnum::Admin, vec!["bravo-irl"]),
        (WorkspaceRoleEnum::Owner, vec!["delta-own"]),
    ] {
        let (slugs, total) = page(
            MemberFilter {
                role: Some(role),
                ..by_name
            },
            25,
            0,
        )
        .await;
        assert_eq!(
            (slugs, total),
            (
                expected.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                expected.len() as i64
            )
        );
    }

    // Their switchers, from every workspace they are in and no other; or a few of them.
    let all = workspaces::member_switchers(&pool, manager, None)
        .await
        .unwrap();
    let mut seen: Vec<_> = all.iter().map(|s| s.workspace_slug.as_str()).collect();
    seen.sort();
    assert_eq!(
        seen,
        ["alpha-live", "bravo-irl", "charlie-irl", "delta-own"]
    );
    assert_eq!(
        all.iter()
            .find(|s| s.workspace_slug == "bravo-irl")
            .unwrap()
            .role,
        WorkspaceRoleEnum::Admin
    );
    let some = workspaces::member_switchers(&pool, manager, Some(&[own, hidden]))
        .await
        .unwrap();
    assert_eq!(some.len(), 1, "a workspace they aren't in is never read");
    assert_eq!(some[0].workspace_id, own);
}
