//! The admin section's data: prices, products, and the staff read models.

use chrono::{TimeDelta, Utc};
use db::{
    admin,
    billing::{self, AddonFields, AddonVisibility, ItemKind, ItemSync, SubscriptionSync},
    plans::{self, PlanFields},
    prices, users, workspaces,
};
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn a_new_price_archives_the_old_one_which_stays_recognised(pool: PgPool) {
    let replaced = prices::set_active(&pool, ItemKind::Plan, "pro", "month", 900, "eur", "price_a")
        .await
        .unwrap();
    assert!(replaced.is_none());

    let replaced = prices::set_active(
        &pool,
        ItemKind::Plan,
        "pro",
        "month",
        1200,
        "eur",
        "price_b",
    )
    .await
    .unwrap()
    .expect("the €9 price is replaced");
    assert_eq!(replaced.stripe_price_id, "price_a");

    let active = prices::active(&pool, ItemKind::Plan, "pro", "month")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (active.stripe_price_id.as_str(), active.amount),
        ("price_b", 1200)
    );

    // Both are still known, so a subscription on the old price keeps mapping to Pro.
    let all = prices::all(&pool).await.unwrap();
    assert_eq!(all.len(), 2);
    assert!(
        all.iter()
            .any(|p| p.stripe_price_id == "price_a" && !p.active)
    );

    // Linking the same Stripe price again doesn't duplicate it.
    prices::set_active(
        &pool,
        ItemKind::Plan,
        "pro",
        "month",
        1200,
        "eur",
        "price_b",
    )
    .await
    .unwrap();
    assert_eq!(prices::all(&pool).await.unwrap().len(), 2);

    // Stop selling yearly (nothing to archive) and monthly.
    assert!(
        prices::archive(&pool, ItemKind::Plan, "pro", "year")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        prices::archive(&pool, ItemKind::Plan, "pro", "month")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        prices::active(&pool, ItemKind::Plan, "pro", "month")
            .await
            .unwrap()
            .is_none()
    );
    assert!(prices::currencies_in_use(&pool).await.unwrap().is_empty());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn plans_and_addons_are_created_and_edited(pool: PgPool) {
    let fields = PlanFields {
        name: "Team",
        description: "For crews",
        is_active: true,
        position: 5,
        max_switchers: 20,
        max_stream_servers: 8,
        max_workspace_members: 30,
        history_days: 180,
        custom_chat_bot: false,
    };
    let team = plans::create(&pool, "team", &fields).await.unwrap();
    assert_eq!(team.max_switchers, 20);
    assert_eq!(team.max_stream_servers, 8);
    assert!(
        plans::create(&pool, "team", &fields)
            .await
            .unwrap_err()
            .is_unique_violation()
    );

    let hidden = PlanFields {
        is_active: false,
        ..fields.clone()
    };
    plans::update(&pool, "team", &hidden)
        .await
        .unwrap()
        .unwrap();
    plans::set_stripe_product(&pool, "team", "prod_team")
        .await
        .unwrap();
    let all = plans::list_all(&pool).await.unwrap();
    assert_eq!(
        all.iter().map(|p| p.code.as_str()).collect::<Vec<_>>(),
        ["free", "pro", "team"]
    );
    assert_eq!(all[2].stripe_product_id.as_deref(), Some("prod_team"));
    assert!(
        plans::list_active(&pool)
            .await
            .unwrap()
            .iter()
            .all(|p| p.code != "team")
    );

    let addon = billing::create_addon(
        &pool,
        "backup",
        &AddonFields {
            name: "Backup ingest",
            description: "A second path",
            unit: "path",
            max_quantity: 2,
            status: AddonVisibility::Available,
            position: 1,
        },
    )
    .await
    .unwrap();
    assert_eq!(addon.unit, "path");
    billing::set_addon_stripe_product(&pool, "backup", "prod_backup")
        .await
        .unwrap();
    let listed = billing::list_addons(&pool).await.unwrap();
    assert_eq!(
        listed.iter().map(|a| a.code.as_str()).collect::<Vec<_>>(),
        ["ingest", "backup"]
    );
    assert_eq!(listed[1].stripe_product_id.as_deref(), Some("prod_backup"));
}

async fn user_named(pool: &PgPool, name: &str) -> Uuid {
    let user = users::create(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO oauth2_connections (user_id, provider, access_token, provider_id, provider_username)
         VALUES ($1, 'TWITCH', 'token', $2, $2)",
    )
    .bind(user.id)
    .bind(name)
    .execute(pool)
    .await
    .unwrap();
    user.id
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn users_and_workspaces_are_searched_and_paged(pool: PgPool) {
    let kate = user_named(&pool, "kate_irl").await;
    let bob = user_named(&pool, "bob").await;
    workspaces::create(&pool, kate, "Kate Live", "kate-live")
        .await
        .unwrap();
    workspaces::create(&pool, bob, "Bike Crew", "bike-crew")
        .await
        .unwrap();

    let search = |query| admin::UserFilter {
        query: Some(query),
        ..Default::default()
    };
    let (found, total) = admin::list_users(&pool, search("KATE"), 10, 0)
        .await
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(found[0].username.as_deref(), Some("kate_irl"));
    assert_eq!(found[0].workspaces, 1);
    let (page, total) = admin::list_users(&pool, admin::UserFilter::default(), 1, 1)
        .await
        .unwrap();
    assert_eq!((page.len(), total), (1, 2));

    // Kate signed in a month ago, Bob just now (the trigger would stamp now()).
    sqlx::query("ALTER TABLE oauth2_connections DISABLE TRIGGER USER")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE oauth2_connections SET updated_at = now() - interval '30 days' WHERE user_id = $1",
    )
    .bind(kate)
    .execute(&pool)
    .await
    .unwrap();
    let week_ago = Utc::now() - TimeDelta::days(7);
    let (active, total) = admin::list_users(
        &pool,
        admin::UserFilter {
            active_since: Some(week_ago),
            ..Default::default()
        },
        10,
        0,
    )
    .await
    .unwrap();
    assert_eq!((active[0].id, total), (bob, 1));
    let (inactive, total) = admin::list_users(
        &pool,
        admin::UserFilter {
            inactive_since: Some(week_ago),
            ..Default::default()
        },
        10,
        0,
    )
    .await
    .unwrap();
    assert_eq!((inactive[0].id, total), (kate, 1));
    let (by_activity, _) = admin::list_users(
        &pool,
        admin::UserFilter {
            by_activity: true,
            ..Default::default()
        },
        10,
        0,
    )
    .await
    .unwrap();
    assert_eq!(
        by_activity.iter().map(|user| user.id).collect::<Vec<_>>(),
        [bob, kate]
    );

    let (found, _) = admin::list_workspaces(&pool, Some("bike"), None, 10, 0)
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].owner_username.as_deref(), Some("bob"));
    assert_eq!(found[0].members, 1);
    let (_, total) = admin::list_workspaces(&pool, None, None, 10, 0)
        .await
        .unwrap();
    assert_eq!(total, 2);
    assert!(
        admin::get_workspace(&pool, found[0].id)
            .await
            .unwrap()
            .is_some()
    );

    assert!(admin::set_staff(&pool, kate, true).await.unwrap());
    assert!(admin::get_user(&pool, kate).await.unwrap().unwrap().staff);
    assert!(!admin::set_staff(&pool, Uuid::new_v4(), true).await.unwrap());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn the_overview_counts_payers_and_monthly_revenue(pool: PgPool) {
    let owner = user_named(&pool, "kate").await;
    let workspace = workspaces::create(&pool, owner, "Kate Live", "kate-live")
        .await
        .unwrap();
    prices::set_active(
        &pool,
        ItemKind::Plan,
        "pro",
        "year",
        9000,
        "eur",
        "price_pro_y",
    )
    .await
    .unwrap();
    prices::set_active(
        &pool,
        ItemKind::Addon,
        "ingest",
        "year",
        6000,
        "eur",
        "price_ingest_y",
    )
    .await
    .unwrap();
    let now = Utc::now();
    billing::apply_subscription(
        &pool,
        &SubscriptionSync {
            workspace_id: workspace.id,
            stripe_subscription_id: "sub_1".into(),
            billing_interval: "year".into(),
            status: "active".into(),
            current_period_start: Some(now),
            current_period_end: Some(now + TimeDelta::days(365)),
            cancel_at_period_end: false,
            canceled_at: None,
            items: vec![
                ItemSync {
                    stripe_item_id: "si_1".into(),
                    stripe_price_id: "price_pro_y".into(),
                    kind: ItemKind::Plan,
                    code: "pro".into(),
                    quantity: 1,
                },
                ItemSync {
                    stripe_item_id: "si_2".into(),
                    stripe_price_id: "price_ingest_y".into(),
                    kind: ItemKind::Addon,
                    code: "ingest".into(),
                    quantity: 2,
                },
            ],
        },
    )
    .await
    .unwrap();

    let overview = admin::overview(&pool).await.unwrap();
    assert_eq!(overview.counts.users, 1);
    assert_eq!(overview.counts.paying_workspaces, 1);
    // (90 + 2 × 60) € a year = 17.50 € a month.
    assert_eq!(
        overview.mrr,
        [admin::Mrr {
            currency: "eur".into(),
            amount: 1750
        }]
    );
    let pro = overview.plans.iter().find(|p| p.code == "pro").unwrap();
    assert_eq!(pro.workspaces, 1);
    assert_eq!(overview.addons[0].units, 2);

    let subscribers = admin::subscribers(&pool).await.unwrap();
    assert_eq!(subscribers.len(), 2);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn every_addon_status_is_stored(pool: PgPool) {
    for status in [
        AddonVisibility::Hidden,
        AddonVisibility::ComingSoon,
        AddonVisibility::Unavailable,
        AddonVisibility::Available,
    ] {
        let fields = AddonFields {
            name: "Ingest",
            description: "",
            unit: "slot",
            max_quantity: 5,
            status,
            position: 0,
        };
        let addon = billing::update_addon(&pool, "ingest", &fields)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(addon.status, status);
    }
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn only_unused_products_can_be_removed(pool: PgPool) {
    let owner = user_named(&pool, "kate").await;
    let workspace = workspaces::create(&pool, owner, "Kate Live", "kate-live")
        .await
        .unwrap();

    // Free has workspaces on it.
    assert_eq!(
        admin::remove_product(&pool, ItemKind::Plan, "free")
            .await
            .unwrap(),
        Err(admin::InUse::Workspaces(1))
    );

    // Ingest is paid for by a live subscription.
    prices::set_active(
        &pool,
        ItemKind::Addon,
        "ingest",
        "month",
        500,
        "eur",
        "price_old",
    )
    .await
    .unwrap();
    prices::set_active(
        &pool,
        ItemKind::Addon,
        "ingest",
        "month",
        600,
        "eur",
        "price_new",
    )
    .await
    .unwrap();
    let now = Utc::now();
    let mut sync = SubscriptionSync {
        workspace_id: workspace.id,
        stripe_subscription_id: "sub_1".into(),
        billing_interval: "month".into(),
        status: "active".into(),
        current_period_start: Some(now),
        current_period_end: Some(now + TimeDelta::days(30)),
        cancel_at_period_end: false,
        canceled_at: None,
        items: vec![ItemSync {
            stripe_item_id: "si_1".into(),
            stripe_price_id: "price_old".into(),
            kind: ItemKind::Addon,
            code: "ingest".into(),
            quantity: 1,
        }],
    };
    billing::apply_subscription(&pool, &sync).await.unwrap();
    assert_eq!(
        admin::remove_product(&pool, ItemKind::Addon, "ingest")
            .await
            .unwrap(),
        Err(admin::InUse::Subscriptions(1))
    );

    // Once the subscription is canceled nothing depends on it: it goes, with its
    // prices, and only the active one needs archiving in Stripe.
    sync.status = "canceled".into();
    billing::apply_subscription(&pool, &sync).await.unwrap();
    assert!(
        admin::in_use(&pool, ItemKind::Addon, "ingest")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        admin::remove_product(&pool, ItemKind::Addon, "ingest")
            .await
            .unwrap(),
        Ok(vec!["price_new".to_owned()])
    );
    assert!(billing::get_addon(&pool, "ingest").await.unwrap().is_none());
    assert!(prices::all(&pool).await.unwrap().is_empty());

    // An unused plan goes too.
    assert_eq!(
        admin::remove_product(&pool, ItemKind::Plan, "pro")
            .await
            .unwrap(),
        Ok(vec![])
    );
    assert!(plans::get_by_code(&pool, "pro").await.unwrap().is_none());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn the_status_page_finds_switchers_chat_counts_and_revoked_grants(pool: PgPool) {
    use db::chat::{self, ChatPlatform, Grant, PlatformAccount};

    let kate = user_named(&pool, "kate_irl").await;
    let workspace = workspaces::create(&pool, kate, "Kate Live", "kate-live")
        .await
        .unwrap();
    let switcher = db::switchers::create(&pool, workspace.id, "Main", b"a")
        .await
        .unwrap();

    let found = admin::switchers_by_ids(&pool, &[switcher.id, Uuid::new_v4()])
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].name.as_str(), found[0].workspace_slug.as_str()),
        ("Main", "kate-live")
    );

    let account = |platform, id, login| PlatformAccount {
        platform,
        id,
        login,
        display_name: login,
    };
    chat::link_channel(
        &pool,
        switcher.id,
        account(ChatPlatform::Twitch, "100", "kate"),
        Grant::default(),
        kate,
    )
    .await
    .unwrap();
    chat::link_channel(
        &pool,
        switcher.id,
        account(ChatPlatform::Kick, "200", "kate_kick"),
        Grant::default(),
        kate,
    )
    .await
    .unwrap();
    chat::upsert_bot(
        &pool,
        Some(workspace.id),
        account(ChatPlatform::Twitch, "4", "katebot"),
        Grant::default(),
        kate,
    )
    .await
    .unwrap();
    chat::record_subscription(
        &pool,
        ChatPlatform::Kick,
        "chat.message.sent",
        "200",
        "",
        "k1",
    )
    .await
    .unwrap();

    let counts = admin::chat_counts(&pool).await.unwrap();
    assert_eq!(
        (
            counts.switchers,
            counts.active_channels,
            counts.links,
            counts.twitch_subscriptions,
            counts.kick_subscriptions
        ),
        (1, 2, 2, 0, 1)
    );
    assert!(admin::revoked_grants(&pool).await.unwrap().is_empty());

    chat::revoke_account(&pool, ChatPlatform::Twitch, "100")
        .await
        .unwrap();
    chat::revoke_account(&pool, ChatPlatform::Twitch, "4")
        .await
        .unwrap();
    let revoked = admin::revoked_grants(&pool).await.unwrap();
    let mut summary = revoked
        .iter()
        .map(|grant| {
            (
                grant.kind.as_str(),
                grant.login.as_str(),
                grant.workspace_name.as_deref(),
                grant.switchers,
            )
        })
        .collect::<Vec<_>>();
    summary.sort();
    assert_eq!(
        summary,
        [
            ("bot", "katebot", Some("Kate Live"), 0),
            ("channel", "kate", Some("Kate Live"), 1)
        ]
    );
    assert_eq!(admin::chat_counts(&pool).await.unwrap().active_channels, 1);
}
