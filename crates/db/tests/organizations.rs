//! Database tests for organizations: client workspaces, the pooled limits and
//! the access of an organization's team.

use chrono::{Duration, Utc};
use db::{
    chat::{self, ChatPlatform, Grant, PlatformAccount},
    invites,
    organizations::{
        self, AcceptOutcome, LimitUpdate, Limits, OrganizationPermission, OrganizationRole,
    },
    plans,
    switchers::{self},
    users,
    workspaces::{self, WorkspacePermission, WorkspaceRoleEnum},
};
use sqlx::PgPool;
use uuid::Uuid;

const LIMITS: Limits = Limits {
    max_switchers: 3,
    max_stream_servers: 7,
    max_workspace_members: 2,
    history_days: 45,
};

struct Setup {
    organization: Uuid,
    staff: Uuid,
    admin: Uuid,
    team_member: Uuid,
}

/// An organization with an admin and a plain team member.
async fn organization(pool: &PgPool) -> Setup {
    let staff = users::create(pool).await.unwrap();
    let admin = users::create(pool).await.unwrap();
    let team_member = users::create(pool).await.unwrap();
    let organization =
        organizations::create(pool, "Nomad Hosting", "nomad-hosting", LIMITS, staff.id)
            .await
            .unwrap();
    organizations::add_member(pool, organization.id, admin.id, OrganizationRole::Admin)
        .await
        .unwrap();
    organizations::add_member(
        pool,
        organization.id,
        team_member.id,
        OrganizationRole::Member,
    )
    .await
    .unwrap();

    Setup {
        organization: organization.id,
        staff: staff.id,
        admin: admin.id,
        team_member: team_member.id,
    }
}

async fn client(pool: &PgPool, setup: &Setup, slug: &str, allocation: Option<i32>) -> Uuid {
    organizations::create_client(
        pool,
        setup.organization,
        setup.admin,
        slug,
        slug,
        allocation,
    )
    .await
    .unwrap()
    .id
}

async fn add_switcher(pool: &PgPool, workspace: Uuid, name: &str) -> Option<Uuid> {
    switchers::create_within_plan(pool, workspace, name, name.as_bytes())
        .await
        .unwrap()
        .map(|switcher| switcher.id)
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn a_client_workspace_has_the_organizations_limits(pool: PgPool) {
    let setup = organization(&pool).await;
    let hosted = client(&pool, &setup, "streamer-one", None).await;

    let plan = plans::for_workspace(&pool, hosted).await.unwrap().unwrap();
    assert_eq!(plan.code, "hosted");
    assert_eq!(plan.max_switchers, 3);
    assert_eq!(plan.max_stream_servers, 7);
    assert_eq!(plan.max_workspace_members, 2);
    assert_eq!(plan.history_days, 45);
    // Customers' own chat bots are the organization's call, off until it allows them.
    assert!(!plan.custom_chat_bot);
    organizations::set_custom_chat_bot(&pool, setup.organization, true)
        .await
        .unwrap()
        .unwrap();
    assert!(
        plans::for_workspace(&pool, hosted)
            .await
            .unwrap()
            .unwrap()
            .custom_chat_bot
    );

    // A normal workspace still gets its plan.
    let owner = users::create(&pool).await.unwrap();
    let own = workspaces::create(&pool, owner.id, "Own", "own")
        .await
        .unwrap();
    let plan = plans::for_workspace(&pool, own.id).await.unwrap().unwrap();
    assert_eq!(plan.code, "free");
    assert_eq!(plan.max_switchers, 1);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn an_allocation_caps_a_client_within_the_pool(pool: PgPool) {
    let setup = organization(&pool).await;
    let capped = client(&pool, &setup, "capped", Some(1)).await;
    // More than the pool: the pool wins.
    let generous = client(&pool, &setup, "generous", Some(10)).await;

    assert!(add_switcher(&pool, capped, "a").await.is_some());
    assert!(add_switcher(&pool, capped, "b").await.is_none());

    assert!(add_switcher(&pool, generous, "c").await.is_some());
    assert!(add_switcher(&pool, generous, "d").await.is_some());
    // The pool (3) is used up across both workspaces.
    assert!(add_switcher(&pool, generous, "e").await.is_none());
    assert!(add_switcher(&pool, capped, "f").await.is_none());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn a_switcher_limit_cannot_go_below_the_switchers_in_use(pool: PgPool) {
    let setup = organization(&pool).await;
    let one = client(&pool, &setup, "one", None).await;
    let two = client(&pool, &setup, "two", None).await;
    let first = add_switcher(&pool, one, "first").await.unwrap();
    let second = add_switcher(&pool, two, "second").await.unwrap();
    let third = add_switcher(&pool, one, "third").await.unwrap();

    // The pool (3) is all in use: 2 is refused and nothing changes ...
    let smaller = Limits {
        max_switchers: 2,
        ..LIMITS
    };
    assert!(matches!(
        organizations::update(&pool, setup.organization, "Nomad Hosting", smaller)
            .await
            .unwrap(),
        LimitUpdate::BelowUsage { used: 3 }
    ));
    let kept = organizations::get_by_id(&pool, setup.organization)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(kept.max_switchers, 3);
    // ... exactly the usage is fine, and so is lowering what only blocks new ones.
    let tighter = Limits {
        max_stream_servers: 1,
        max_workspace_members: 1,
        history_days: 7,
        ..LIMITS
    };
    assert!(matches!(
        organizations::update(&pool, setup.organization, "Nomad Hosting", tighter)
            .await
            .unwrap(),
        LimitUpdate::Updated(_)
    ));

    // The same for a client's allocation: it has 2 switchers.
    assert!(matches!(
        organizations::update_client(&pool, setup.organization, one, "one", Some(1))
            .await
            .unwrap(),
        LimitUpdate::BelowUsage { used: 2 }
    ));
    let updated =
        organizations::update_client(&pool, setup.organization, one, "one", Some(2)).await;
    assert!(matches!(updated.unwrap(), LimitUpdate::Updated(_)));
    // No cap at all is always fine.
    let uncapped = organizations::update_client(&pool, setup.organization, one, "one", None).await;
    assert!(matches!(uncapped.unwrap(), LimitUpdate::Updated(_)));

    // A limit never pauses anything, and a full pool only stops new switchers.
    for id in [first, second, third] {
        assert!(!switchers::is_paused(&pool, id).await.unwrap());
    }
    assert!(add_switcher(&pool, two, "fourth").await.is_none());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn suspending_the_organization_pauses_every_client(pool: PgPool) {
    let setup = organization(&pool).await;
    let one = client(&pool, &setup, "one", None).await;
    let switcher = add_switcher(&pool, one, "a").await.unwrap();
    assert!(!switchers::is_paused(&pool, switcher).await.unwrap());

    organizations::set_suspension(&pool, setup.organization, Some("unpaid"))
        .await
        .unwrap();
    assert!(switchers::is_paused(&pool, switcher).await.unwrap());
    assert!(switchers::list_summaries(&pool, one).await.unwrap()[0].paused);
    let access =
        workspaces::member_access(&pool, one, setup.admin, WorkspacePermission::SWITCHER_VIEW)
            .await
            .unwrap();
    assert_eq!(access.suspended_reason.as_deref(), Some("unpaid"));

    organizations::set_suspension(&pool, setup.organization, None)
        .await
        .unwrap();
    assert!(!switchers::is_paused(&pool, switcher).await.unwrap());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn the_team_gets_workspace_permissions_by_organization_role(pool: PgPool) {
    let setup = organization(&pool).await;
    let hosted = client(&pool, &setup, "streamer-one", None).await;
    let stranger = users::create(&pool).await.unwrap();

    let allowed = |user: Uuid, permission: WorkspacePermission| {
        let pool = pool.clone();
        async move {
            let access = workspaces::member_access(&pool, hosted, user, permission)
                .await
                .unwrap();
            assert!(
                !access.allowed,
                "no one is a member of the workspace itself"
            );
            access.organization_allowed
        }
    };

    for permission in [
        WorkspacePermission::ACCOUNT_VIEW,
        WorkspacePermission::ACCOUNT_MANAGE,
        WorkspacePermission::SWITCHER_VIEW,
        WorkspacePermission::SWITCHER_CONTROL,
        WorkspacePermission::SWITCHER_MANAGE,
    ] {
        assert!(allowed(setup.admin, permission).await);
        assert!(allowed(setup.team_member, permission).await);
        assert!(!allowed(stranger.id, permission).await);
    }
    // Only admins delete, nobody pays or hands over.
    assert!(allowed(setup.admin, WorkspacePermission::WORKSPACE_DELETE).await);
    assert!(!allowed(setup.team_member, WorkspacePermission::WORKSPACE_DELETE).await);
    for user in [setup.admin, setup.team_member] {
        assert!(!allowed(user, WorkspacePermission::BILLING_MANAGE).await);
        assert!(!allowed(user, WorkspacePermission::WORKSPACE_TRANSFER).await);
    }

    let access = workspaces::member_access(
        &pool,
        hosted,
        setup.admin,
        WorkspacePermission::SWITCHER_VIEW,
    )
    .await
    .unwrap();
    assert_eq!(access.organization_id, Some(setup.organization));
    assert_eq!(access.organization_name.as_deref(), Some("Nomad Hosting"));

    // An ordinary workspace has no organization.
    let owner = users::create(&pool).await.unwrap();
    let own = workspaces::create(&pool, owner.id, "Own", "own")
        .await
        .unwrap();
    let access = workspaces::member_access(
        &pool,
        own.id,
        setup.admin,
        WorkspacePermission::SWITCHER_VIEW,
    )
    .await
    .unwrap();
    assert!(!access.organization_allowed && access.organization_id.is_none());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn organization_permissions_follow_the_role(pool: PgPool) {
    let setup = organization(&pool).await;
    let can = |user, permission| {
        let pool = pool.clone();
        async move {
            organizations::has_permission(&pool, setup.organization, user, permission)
                .await
                .unwrap()
        }
    };

    assert!(can(setup.admin, OrganizationPermission::MANAGE).await);
    assert!(can(setup.admin, OrganizationPermission::MEMBERS).await);
    assert!(can(setup.team_member, OrganizationPermission::VIEW).await);
    assert!(!can(setup.team_member, OrganizationPermission::MANAGE).await);
    assert!(!can(setup.team_member, OrganizationPermission::MEMBERS).await);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn a_customer_joins_a_client_workspace_as_admin_within_its_limit(pool: PgPool) {
    let setup = organization(&pool).await;
    let hosted = client(&pool, &setup, "streamer-one", None).await;
    let expires = Utc::now() + Duration::days(7);

    // The organization works in it before anyone joins.
    assert_eq!(
        organizations::get_client(&pool, setup.organization, hosted)
            .await
            .unwrap()
            .unwrap()
            .member_count,
        0
    );

    for (index, token) in [b"one".as_slice(), b"two", b"three"]
        .into_iter()
        .enumerate()
    {
        invites::create(
            &pool,
            hosted,
            organizations::CUSTOMER_ROLE,
            setup.admin,
            token,
            expires,
        )
        .await
        .unwrap();
        let customer = users::create(&pool).await.unwrap();
        let outcome = invites::accept(&pool, token, customer.id).await.unwrap();
        if index < 2 {
            assert!(
                matches!(outcome, invites::AcceptOutcome::Joined { .. }),
                "{outcome:?}"
            );
            assert_eq!(
                workspaces::member_role(&pool, hosted, customer.id)
                    .await
                    .unwrap(),
                Some(WorkspaceRoleEnum::Admin)
            );
        } else {
            // The organization's member limit (2) per client.
            assert_eq!(outcome, invites::AcceptOutcome::MemberLimitReached);
        }
    }

    let client = organizations::get_client(&pool, setup.organization, hosted)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(client.member_count, 2);

    let preview = invites::preview_by_token_hash(&pool, b"one")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(preview.organization_name.as_deref(), Some("Nomad Hosting"));
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn a_client_workspace_has_no_plan_to_change(pool: PgPool) {
    let setup = organization(&pool).await;
    let hosted = client(&pool, &setup, "streamer-one", None).await;

    assert!(
        workspaces::set_plan(&pool, hosted, "pro")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        plans::for_workspace(&pool, hosted)
            .await
            .unwrap()
            .unwrap()
            .code,
        "hosted"
    );
    assert!(
        !plans::workspace_ids(&pool, "free")
            .await
            .unwrap()
            .contains(&hosted)
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn clients_belong_to_their_organization(pool: PgPool) {
    let setup = organization(&pool).await;
    let hosted = client(&pool, &setup, "streamer-one", None).await;
    let other = organizations::create(&pool, "Other", "other", LIMITS, setup.admin)
        .await
        .unwrap();

    // Another organization can't change it.
    assert!(matches!(
        organizations::update_client(&pool, other.id, hosted, "Mine now", None)
            .await
            .unwrap(),
        LimitUpdate::NotFound
    ));
    let LimitUpdate::Updated(updated) =
        organizations::update_client(&pool, setup.organization, hosted, "Renamed", Some(2))
            .await
            .unwrap()
    else {
        panic!("the client's own organization can change it");
    };
    assert_eq!(
        (updated.name.as_str(), updated.switcher_allocation),
        ("Renamed", Some(2))
    );

    assert_eq!(
        organizations::workspace_ids(&pool, setup.organization)
            .await
            .unwrap(),
        [hosted]
    );
    // Not deletable while it has clients.
    assert!(
        !organizations::delete_if_empty(&pool, setup.organization)
            .await
            .unwrap()
    );
    assert!(
        organizations::delete_if_empty(&pool, other.id)
            .await
            .unwrap()
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn the_last_admin_cannot_be_removed(pool: PgPool) {
    let setup = organization(&pool).await;

    assert!(
        !organizations::remove_member(&pool, setup.organization, setup.admin)
            .await
            .unwrap()
    );
    assert!(
        organizations::remove_member(&pool, setup.organization, setup.team_member)
            .await
            .unwrap()
    );

    let second_admin = users::create(&pool).await.unwrap();
    organizations::add_member(
        &pool,
        setup.organization,
        second_admin.id,
        OrganizationRole::Admin,
    )
    .await
    .unwrap();
    assert!(
        organizations::remove_member(&pool, setup.organization, setup.admin)
            .await
            .unwrap()
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn team_invites_are_single_use(pool: PgPool) {
    let setup = organization(&pool).await;
    let expires = Utc::now() + Duration::days(7);
    organizations::create_invite(
        &pool,
        setup.organization,
        OrganizationRole::Member,
        None,
        b"tok",
        expires,
    )
    .await
    .unwrap();

    let newcomer = users::create(&pool).await.unwrap();
    let preview = organizations::preview_invite(&pool, b"tok", newcomer.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(preview.organization_slug, "nomad-hosting");
    assert!(!preview.already_member);

    assert!(matches!(
        organizations::accept_invite(&pool, b"tok", newcomer.id)
            .await
            .unwrap(),
        AcceptOutcome::Joined { .. }
    ));
    assert_eq!(
        organizations::member_role(&pool, setup.organization, newcomer.id)
            .await
            .unwrap(),
        Some(OrganizationRole::Member)
    );

    let second = users::create(&pool).await.unwrap();
    assert_eq!(
        organizations::accept_invite(&pool, b"tok", second.id)
            .await
            .unwrap(),
        AcceptOutcome::AlreadyUsed
    );
    assert_eq!(
        organizations::accept_invite(&pool, b"nope", second.id)
            .await
            .unwrap(),
        AcceptOutcome::NotFound
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn an_ordinary_workspace_still_pauses_beyond_its_plan(pool: PgPool) {
    let owner = users::create(&pool).await.unwrap();
    let own = workspaces::create(&pool, owner.id, "Own", "own")
        .await
        .unwrap();
    workspaces::set_plan(&pool, own.id, "pro").await.unwrap();
    let first = add_switcher(&pool, own.id, "first").await.unwrap();
    let second = add_switcher(&pool, own.id, "second").await.unwrap();

    workspaces::set_plan(&pool, own.id, "free").await.unwrap();
    assert!(!switchers::is_paused(&pool, first).await.unwrap());
    assert!(switchers::is_paused(&pool, second).await.unwrap());
}

/// One page of `list_clients`: the slugs on it and the total.
async fn client_page(
    pool: &PgPool,
    setup: &Setup,
    filter: organizations::ClientFilter<'_>,
    limit: i64,
    offset: i64,
) -> (Vec<String>, i64) {
    let (clients, total) =
        organizations::list_clients(pool, setup.organization, filter, limit, offset)
            .await
            .unwrap();
    (
        clients.into_iter().map(|client| client.url_slug).collect(),
        total,
    )
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn clients_are_searched_filtered_and_paged(pool: PgPool) {
    let setup = organization(&pool).await;
    let alpha = client(&pool, &setup, "alpha-live", None).await;
    let bravo = client(&pool, &setup, "bravo-irl", None).await;
    client(&pool, &setup, "charlie-irl", None).await;
    let by_name = organizations::ClientFilter {
        by_name: true,
        ..Default::default()
    };
    assert_eq!(
        organizations::client_count(&pool, setup.organization)
            .await
            .unwrap(),
        3
    );

    // Search by name or slug in any case; an ID finds exactly one.
    let search = organizations::ClientFilter {
        search: Some("IRL"),
        ..by_name
    };
    let (slugs, total) = client_page(&pool, &setup, search, 25, 0).await;
    assert_eq!(slugs, ["bravo-irl", "charlie-irl"]);
    assert_eq!(total, 2);
    let id = alpha.to_string();
    let search = organizations::ClientFilter {
        search: Some(&id),
        ..by_name
    };
    let (slugs, total) = client_page(&pool, &setup, search, 25, 0).await;
    assert_eq!(slugs, ["alpha-live"]);
    assert_eq!(total, 1);

    // Paged: the total counts every match, a page only its share.
    let (slugs, total) = client_page(&pool, &setup, by_name, 2, 0).await;
    assert_eq!(slugs, ["alpha-live", "bravo-irl"]);
    assert_eq!(total, 3);
    let (slugs, total) = client_page(&pool, &setup, by_name, 2, 2).await;
    assert_eq!(slugs, ["charlie-irl"]);
    assert_eq!(total, 3);

    // Newest first unless sorted by name.
    let newest = organizations::ClientFilter::default();
    let (slugs, _) = client_page(&pool, &setup, newest, 25, 0).await;
    assert_eq!(slugs, ["charlie-irl", "bravo-irl", "alpha-live"]);

    // Whether a customer joined.
    invites::create(
        &pool,
        bravo,
        organizations::CUSTOMER_ROLE,
        setup.admin,
        b"bravo",
        Utc::now() + Duration::days(7),
    )
    .await
    .unwrap();
    let customer = users::create(&pool).await.unwrap();
    invites::accept(&pool, b"bravo", customer.id).await.unwrap();
    let joined = organizations::ClientFilter {
        claimed: Some(true),
        ..by_name
    };
    let (slugs, total) = client_page(&pool, &setup, joined, 25, 0).await;
    assert_eq!(slugs, ["bravo-irl"]);
    assert_eq!(total, 1);
    let unclaimed = organizations::ClientFilter {
        claimed: Some(false),
        ..by_name
    };
    let (slugs, total) = client_page(&pool, &setup, unclaimed, 25, 0).await;
    assert_eq!(slugs, ["alpha-live", "charlie-irl"]);
    assert_eq!(total, 2);

    // One client, and only of its own organization; switchers only of those asked for.
    assert!(
        organizations::get_client(&pool, Uuid::new_v4(), alpha)
            .await
            .unwrap()
            .is_none()
    );
    let switcher = add_switcher(&pool, bravo, "main").await.unwrap();
    assert_eq!(
        organizations::client_switchers(&pool, setup.organization, &[bravo])
            .await
            .unwrap(),
        [(bravo, switcher)]
    );
    assert!(
        organizations::client_switchers(&pool, setup.organization, &[alpha])
            .await
            .unwrap()
            .is_empty()
    );
}

fn twitch(id: &'static str, login: &'static str) -> PlatformAccount<'static> {
    PlatformAccount {
        platform: ChatPlatform::Twitch,
        id,
        login,
        display_name: login,
    }
}

async fn speaking(pool: &PgPool, switcher: Uuid) -> (Option<String>, bool, bool) {
    let linked = chat::channels_for_switcher(pool, switcher).await.unwrap();
    (
        linked[0].bot_login.clone(),
        linked[0].bot_is_custom,
        linked[0].bot_is_organization,
    )
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn an_organizations_bot_speaks_for_its_clients_before_the_shared_one(pool: PgPool) {
    let setup = organization(&pool).await;
    let hosted = client(&pool, &setup, "streamer-one", None).await;
    let switcher = add_switcher(&pool, hosted, "main").await.unwrap();
    // An ordinary workspace next to it, on the same channel.
    let owner = users::create(&pool).await.unwrap();
    let own = workspaces::create(&pool, owner.id, "Own", "own")
        .await
        .unwrap();
    let own_switcher = switchers::create(&pool, own.id, "Main", b"own")
        .await
        .unwrap()
        .id;
    for (id, user) in [(switcher, setup.admin), (own_switcher, owner.id)] {
        chat::link_channel(&pool, id, twitch("100", "kate"), Grant::default(), user)
            .await
            .unwrap();
    }
    let team = |login, id| twitch(id, login);
    chat::upsert_bot(
        &pool,
        None,
        team("noalbs", "1"),
        Grant::default(),
        setup.admin,
    )
    .await
    .unwrap();

    // Without a bot of its own the organization's clients get the shared one.
    assert_eq!(
        speaking(&pool, switcher).await,
        (Some("noalbs".into()), false, false)
    );

    // The organization's bot speaks for its clients only, and is not the shared one.
    chat::upsert_organization_bot(
        &pool,
        setup.organization,
        team("nomadbot", "2"),
        Grant::default(),
        setup.admin,
    )
    .await
    .unwrap();
    assert_eq!(
        speaking(&pool, switcher).await,
        (Some("nomadbot".into()), false, true)
    );
    assert_eq!(
        speaking(&pool, own_switcher).await,
        (Some("noalbs".into()), false, false)
    );
    assert_eq!(chat::bots(&pool, None).await.unwrap().len(), 1);
    assert_eq!(
        chat::organization_bots(&pool, setup.organization)
            .await
            .unwrap()
            .len(),
        1
    );

    // A customer's own bot is ignored until the organization allows it, then wins.
    chat::upsert_bot(
        &pool,
        Some(hosted),
        team("customerbot", "3"),
        Grant::default(),
        setup.admin,
    )
    .await
    .unwrap();
    assert_eq!(
        speaking(&pool, switcher).await,
        (Some("nomadbot".into()), false, true)
    );
    organizations::set_custom_chat_bot(&pool, setup.organization, true)
        .await
        .unwrap();
    assert_eq!(
        speaking(&pool, switcher).await,
        (Some("customerbot".into()), true, false)
    );

    // Another organization's bot never speaks for these clients.
    let other = organizations::create(&pool, "Other Hosting", "other-hosting", LIMITS, setup.admin)
        .await
        .unwrap();
    chat::upsert_organization_bot(
        &pool,
        other.id,
        team("otherbot", "9"),
        Grant::default(),
        setup.admin,
    )
    .await
    .unwrap();
    assert_eq!(
        speaking(&pool, switcher).await,
        (Some("customerbot".into()), true, false)
    );

    // Forgetting them falls back one step at a time.
    chat::remove_bot(&pool, Some(hosted), ChatPlatform::Twitch)
        .await
        .unwrap();
    assert_eq!(
        speaking(&pool, switcher).await,
        (Some("nomadbot".into()), false, true)
    );
    assert!(
        chat::remove_organization_bot(&pool, setup.organization, ChatPlatform::Twitch)
            .await
            .unwrap()
    );
    assert_eq!(
        speaking(&pool, switcher).await,
        (Some("noalbs".into()), false, false)
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn activity_spans_the_clients_and_only_them(pool: PgPool) {
    let setup = organization(&pool).await;
    let alpha = client(&pool, &setup, "alpha-live", None).await;
    let bravo = client(&pool, &setup, "bravo-irl", None).await;
    let alpha_switcher = add_switcher(&pool, alpha, "main").await.unwrap();
    let bravo_switcher = add_switcher(&pool, bravo, "backup").await.unwrap();
    // An ordinary workspace's events are not the organization's.
    let owner = users::create(&pool).await.unwrap();
    let own = workspaces::create(&pool, owner.id, "Own", "own")
        .await
        .unwrap();
    let own_switcher = switchers::create(&pool, own.id, "Main", b"own")
        .await
        .unwrap()
        .id;

    let event = |switcher, kind: &'static str| {
        let pool = pool.clone();
        async move {
            db::switcher_events::insert(&pool, switcher, kind, serde_json::json!({}))
                .await
                .unwrap()
        }
    };
    event(alpha_switcher, "stream_started").await;
    event(bravo_switcher, "obs_connected").await;
    event(own_switcher, "stream_started").await;
    let newest = event(bravo_switcher, "stream_stopped").await;

    let all = |filter, before, limit| {
        let pool = pool.clone();
        let organization = setup.organization;
        async move {
            organizations::activity(&pool, organization, filter, before, limit)
                .await
                .unwrap()
        }
    };
    let kinds_of = |rows: &[organizations::ActivityRow]| -> Vec<String> {
        rows.iter().map(|row| row.kind.clone()).collect()
    };

    // Newest first across both clients, with where each happened.
    let rows = all(organizations::ActivityFilter::default(), None, 50).await;
    assert_eq!(
        kinds_of(&rows),
        ["stream_stopped", "obs_connected", "stream_started"]
    );
    assert_eq!(rows[0].workspace_slug, "bravo-irl");
    assert_eq!(rows[0].switcher_name, "backup");

    // One client, a name search, a list of kinds.
    let one = organizations::ActivityFilter {
        workspace_id: Some(alpha),
        ..Default::default()
    };
    assert_eq!(kinds_of(&all(one, None, 50).await), ["stream_started"]);
    let by_name = organizations::ActivityFilter {
        search: Some("BRAVO"),
        ..Default::default()
    };
    assert_eq!(all(by_name, None, 50).await.len(), 2);
    let by_switcher = organizations::ActivityFilter {
        search: Some("backup"),
        ..Default::default()
    };
    assert_eq!(all(by_switcher, None, 50).await.len(), 2);
    let kinds = ["stream_started".to_owned(), "stream_stopped".to_owned()];
    let by_kind = organizations::ActivityFilter {
        kinds: Some(&kinds),
        ..Default::default()
    };
    assert_eq!(
        kinds_of(&all(by_kind, None, 50).await),
        ["stream_stopped", "stream_started"]
    );

    // Paging back from the newest.
    let older = all(
        organizations::ActivityFilter::default(),
        Some(newest.created_at),
        1,
    )
    .await;
    assert_eq!(kinds_of(&older), ["obs_connected"]);

    // Another organization sees none of it.
    assert!(
        organizations::activity(
            &pool,
            Uuid::new_v4(),
            organizations::ActivityFilter::default(),
            None,
            50
        )
        .await
        .unwrap()
        .is_empty()
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn staff_see_which_organization_a_workspace_belongs_to(pool: PgPool) {
    let setup = organization(&pool).await;
    let hosted = client(&pool, &setup, "streamer-one", None).await;
    let owner = users::create(&pool).await.unwrap();
    workspaces::create(&pool, owner.id, "Own", "own")
        .await
        .unwrap();

    let (all, total) = db::admin::list_workspaces(&pool, None, None, 25, 0)
        .await
        .unwrap();
    assert_eq!(total, 2);
    let managed = all.iter().find(|workspace| workspace.id == hosted).unwrap();
    assert_eq!(managed.organization_id, Some(setup.organization));
    assert_eq!(managed.organization_name.as_deref(), Some("Nomad Hosting"));
    assert!(
        all.iter()
            .find(|workspace| workspace.url_slug == "own")
            .unwrap()
            .organization_id
            .is_none()
    );

    // Only one organization's clients, also when searching.
    let (only, total) = db::admin::list_workspaces(&pool, None, Some(setup.organization), 25, 0)
        .await
        .unwrap();
    assert_eq!((only.len(), total), (1, 1));
    let (none, total) =
        db::admin::list_workspaces(&pool, Some("own"), Some(setup.organization), 25, 0)
            .await
            .unwrap();
    assert_eq!((none.len(), total), (0, 0));
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn an_organizations_suspension_is_known_as_the_organizations(pool: PgPool) {
    let setup = organization(&pool).await;
    let hosted = client(&pool, &setup, "streamer-one", None).await;
    let customer = users::create(&pool).await.unwrap();
    invites::create(
        &pool,
        hosted,
        organizations::CUSTOMER_ROLE,
        setup.admin,
        b"one",
        Utc::now() + Duration::days(7),
    )
    .await
    .unwrap();
    invites::accept(&pool, b"one", customer.id).await.unwrap();
    let access = |user| {
        let pool = pool.clone();
        async move {
            workspaces::member_access(&pool, hosted, user, WorkspacePermission::SWITCHER_VIEW)
                .await
                .unwrap()
        }
    };

    // The organization's suspension carries its reason, and says whose it is.
    organizations::set_suspension(&pool, setup.organization, Some("unpaid invoice"))
        .await
        .unwrap();
    let as_customer = access(customer.id).await;
    assert!(as_customer.allowed && !as_customer.organization_allowed);
    assert_eq!(
        as_customer.suspended_reason.as_deref(),
        Some("unpaid invoice")
    );
    assert!(as_customer.suspended_by_organization);
    let summary = workspaces::get_summary(&pool, hosted, customer.id)
        .await
        .unwrap()
        .unwrap();
    assert!(summary.suspended_by_organization);
    assert!(summary.organization_role.is_none() && summary.role.is_some());
    assert!(access(setup.team_member).await.organization_allowed);

    // The workspace's own suspension is the workspace's.
    organizations::set_suspension(&pool, setup.organization, None)
        .await
        .unwrap();
    workspaces::set_suspension(&pool, hosted, Some("abuse report"))
        .await
        .unwrap();
    let as_customer = access(customer.id).await;
    assert_eq!(
        as_customer.suspended_reason.as_deref(),
        Some("abuse report")
    );
    assert!(!as_customer.suspended_by_organization);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn the_audit_log_has_the_teams_changes_and_what_staff_did(pool: PgPool) {
    use db::organization_audit::{self, ActorKind, NewEntry};

    let setup = organization(&pool).await;
    let record = |kind, action: &'static str, subject: Option<&'static str>| {
        let pool = pool.clone();
        let (organization, actor) = (setup.organization, setup.admin);
        async move {
            organization_audit::record(
                &pool,
                &NewEntry {
                    organization_id: organization,
                    actor_user_id: actor,
                    actor_name: "kate",
                    actor_kind: kind,
                    action,
                    subject,
                    details: serde_json::json!({ "n": 1 }),
                },
            )
            .await
            .unwrap();
        }
    };
    record(ActorKind::Team, "client_created", Some("Nomad Live")).await;
    record(ActorKind::Customer, "customer_joined", Some("Nomad Live")).await;
    // What staff did to this organization is in it, and what they did to another isn't.
    for (organization, action) in [
        (setup.organization, "suspend_organization"),
        (Uuid::new_v4(), "suspend_organization"),
    ] {
        db::staff_actions::record(
            &pool,
            &db::staff_actions::NewAction {
                staff_user_id: setup.staff,
                staff_name: "someone on staff",
                action,
                workspace_name: Some("Nomad Hosting (organization)"),
                reason: Some("unpaid"),
                details: serde_json::json!({ "organization_id": organization }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    // Another organization has its own log.
    organization_audit::record(
        &pool,
        &NewEntry {
            organization_id: organizations::create(&pool, "Other", "other", LIMITS, setup.staff)
                .await
                .unwrap()
                .id,
            actor_user_id: setup.staff,
            actor_name: "someone else",
            actor_kind: ActorKind::Team,
            action: "client_created",
            subject: None,
            details: serde_json::json!({}),
        },
    )
    .await
    .unwrap();

    let entries = organization_audit::list(&pool, setup.organization, None, 50)
        .await
        .unwrap();
    let seen: Vec<(&str, &str)> = entries
        .iter()
        .map(|entry| (entry.actor_kind.as_str(), entry.action.as_str()))
        .collect();
    assert_eq!(
        seen,
        [
            ("staff", "suspend_organization"),
            ("customer", "customer_joined"),
            ("team", "client_created"),
        ]
    );
    // Staff are NOALBS, with their reason; the others are named as they were.
    assert_eq!(entries[0].actor_name, "NOALBS");
    assert_eq!(entries[0].details["reason"], "unpaid");
    assert_eq!(entries[1].actor_name, "kate");
    assert_eq!(entries[1].subject.as_deref(), Some("Nomad Live"));

    // Paged back from the newest.
    let older =
        organization_audit::list(&pool, setup.organization, Some(entries[1].created_at), 50)
            .await
            .unwrap();
    assert_eq!(older.len(), 1);
    assert_eq!(older[0].action, "client_created");
    // Nothing is old enough to go yet.
    assert_eq!(organization_audit::delete_expired(&pool).await.unwrap(), 0);
}
