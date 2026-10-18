//! Billing: Stripe customers, mirrored subscriptions, the plan they give a
//! workspace, and pausing switchers over the plan's limit.

use chrono::{TimeDelta, Utc};
use db::{
    billing::{self, ItemKind, ItemSync, SubscriptionSync},
    switchers, users, workspaces,
};
use sqlx::PgPool;
use uuid::Uuid;

async fn workspace(pool: &PgPool) -> Uuid {
    let owner = users::create(pool).await.unwrap();
    workspaces::create(pool, owner.id, "Nomad Live", "nomad-live")
        .await
        .unwrap()
        .id
}

fn item(id: &str, kind: ItemKind, code: &str, quantity: i32) -> ItemSync {
    ItemSync {
        stripe_item_id: id.into(),
        stripe_price_id: format!("price_{code}_month"),
        kind,
        code: code.into(),
        quantity,
    }
}

fn subscription(workspace_id: Uuid, status: &str, items: Vec<ItemSync>) -> SubscriptionSync {
    let now = Utc::now();
    SubscriptionSync {
        workspace_id,
        stripe_subscription_id: "sub_1".into(),
        billing_interval: "month".into(),
        status: status.into(),
        current_period_start: Some(now),
        current_period_end: Some(now + TimeDelta::days(30)),
        cancel_at_period_end: false,
        canceled_at: None,
        items,
    }
}

async fn plan_code(pool: &PgPool, workspace_id: Uuid) -> String {
    workspaces::get_by_id(pool, workspace_id)
        .await
        .unwrap()
        .unwrap()
        .plan_code
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn the_live_subscription_decides_the_plan(pool: PgPool) {
    let workspace_id = workspace(&pool).await;
    let pro = || vec![item("si_1", ItemKind::Plan, "pro", 1)];

    let change = billing::apply_subscription(&pool, &subscription(workspace_id, "active", pro()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (change.before.as_str(), change.after.as_str()),
        ("free", "pro")
    );
    assert!(change.changed());

    // A failed payment Stripe is still retrying keeps Pro (the grace period).
    let change = billing::apply_subscription(&pool, &subscription(workspace_id, "past_due", pro()))
        .await
        .unwrap()
        .unwrap();
    assert!(!change.changed());
    let live = billing::live_subscription(&pool, workspace_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(live.status, "past_due");
    assert_eq!(live.plan_code.as_deref(), Some("pro"));

    let change = billing::apply_subscription(&pool, &subscription(workspace_id, "canceled", pro()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(change.after, "free");
    assert_eq!(plan_code(&pool, workspace_id).await, "free");
    assert!(
        billing::live_subscription(&pool, workspace_id)
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn items_are_replaced_and_addons_are_entitlements(pool: PgPool) {
    let workspace_id = workspace(&pool).await;
    let sync = subscription(
        workspace_id,
        "active",
        vec![
            item("si_1", ItemKind::Plan, "pro", 1),
            item("si_2", ItemKind::Addon, "ingest", 2),
        ],
    );
    billing::apply_subscription(&pool, &sync).await.unwrap();

    let entitlements = billing::entitlements(&pool, workspace_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entitlements.plan.code, "pro");
    assert_eq!(entitlements.addon("ingest"), 2);
    assert_eq!(entitlements.addon("other"), 0);

    // The add-on was removed in Stripe: the next sync drops it.
    let sync = subscription(
        workspace_id,
        "active",
        vec![item("si_1", ItemKind::Plan, "pro", 1)],
    );
    billing::apply_subscription(&pool, &sync).await.unwrap();
    let entitlements = billing::entitlements(&pool, workspace_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entitlements.addon("ingest"), 0);

    // Add-ons of a subscription that isn't live don't count.
    let mut sync = subscription(
        workspace_id,
        "unpaid",
        vec![item("si_2", ItemKind::Addon, "ingest", 1)],
    );
    sync.stripe_subscription_id = "sub_old".into();
    billing::apply_subscription(&pool, &sync).await.unwrap();
    let entitlements = billing::entitlements(&pool, workspace_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entitlements.addon("ingest"), 0);
    assert_eq!(entitlements.plan.code, "pro");
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn an_addon_only_subscription_keeps_the_free_plan(pool: PgPool) {
    let workspace_id = workspace(&pool).await;
    let sync = subscription(
        workspace_id,
        "active",
        vec![item("si_1", ItemKind::Addon, "ingest", 1)],
    );
    let change = billing::apply_subscription(&pool, &sync)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(change.after, "free");
    let entitlements = billing::entitlements(&pool, workspace_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entitlements.addon("ingest"), 1);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn racing_checkouts_share_one_customer(pool: PgPool) {
    let workspace_id = workspace(&pool).await;

    let first = billing::set_customer_if_missing(&pool, workspace_id, "cus_a")
        .await
        .unwrap();
    let second = billing::set_customer_if_missing(&pool, workspace_id, "cus_b")
        .await
        .unwrap();
    assert_eq!(first.as_deref(), Some("cus_a"));
    assert_eq!(second.as_deref(), Some("cus_a"));
    assert_eq!(
        billing::workspace_by_customer(&pool, "cus_a")
            .await
            .unwrap(),
        Some(workspace_id)
    );
    assert_eq!(
        billing::set_customer_if_missing(&pool, Uuid::new_v4(), "cus_c")
            .await
            .unwrap(),
        None
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn switchers_over_the_limit_are_paused_until_upgrading(pool: PgPool) {
    let workspace_id = workspace(&pool).await;
    workspaces::set_plan(&pool, workspace_id, "pro")
        .await
        .unwrap();
    let first = switchers::create(&pool, workspace_id, "Main", b"a")
        .await
        .unwrap();
    let second = switchers::create(&pool, workspace_id, "Bike", b"b")
        .await
        .unwrap();
    assert!(!switchers::is_paused(&pool, second.id).await.unwrap());

    // Back to Free (1 switcher): the newer one pauses.
    workspaces::set_plan(&pool, workspace_id, "free")
        .await
        .unwrap();
    assert!(!switchers::is_paused(&pool, first.id).await.unwrap());
    assert!(switchers::is_paused(&pool, second.id).await.unwrap());
    let listed = switchers::list_summaries(&pool, workspace_id)
        .await
        .unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|s| (s.name.as_str(), s.paused))
            .collect::<Vec<_>>(),
        [("Main", false), ("Bike", true)]
    );

    // Deleting the older one frees the slot.
    switchers::delete(&pool, first.id).await.unwrap();
    assert!(!switchers::is_paused(&pool, second.id).await.unwrap());
    assert!(!switchers::is_paused(&pool, Uuid::new_v4()).await.unwrap());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn lowered_addons_and_plans_are_kept_until_the_period_ends(pool: PgPool) {
    let workspace_id = workspace(&pool).await;
    let both = subscription(
        workspace_id,
        "active",
        vec![
            item("si_1", ItemKind::Plan, "pro", 1),
            item("si_2", ItemKind::Addon, "ingest", 3),
        ],
    );
    billing::apply_subscription(&pool, &both).await.unwrap();
    let live = billing::live_subscription(&pool, workspace_id)
        .await
        .unwrap()
        .unwrap();
    let period_end = live.current_period_end.unwrap();

    // Down to 1 ingest slot and no Pro, from the next invoice on.
    billing::set_hold(&pool, live.id, ItemKind::Addon, "ingest", 3, period_end)
        .await
        .unwrap();
    // Lowering again the same period keeps the higher hold.
    billing::set_hold(&pool, live.id, ItemKind::Addon, "ingest", 2, period_end)
        .await
        .unwrap();
    billing::set_hold(&pool, live.id, ItemKind::Plan, "pro", 1, period_end)
        .await
        .unwrap();
    let lowered = subscription(
        workspace_id,
        "active",
        vec![item("si_2", ItemKind::Addon, "ingest", 1)],
    );
    let change = billing::apply_subscription(&pool, &lowered)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(change.after, "pro", "Pro is kept until the period ends");

    let statuses = billing::addon_statuses(&pool, workspace_id).await.unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!((statuses[0].billed, statuses[0].held), (1, Some(3)));
    assert_eq!(statuses[0].quantity(), 3);
    let live = billing::live_subscription(&pool, workspace_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(live.plan_code, None);
    assert_eq!(live.held_plan_code.as_deref(), Some("pro"));

    // Taking it back within the period clears the hold.
    billing::clear_hold(&pool, live.id, ItemKind::Addon, "ingest")
        .await
        .unwrap();
    assert!(
        billing::hold(&pool, live.id, ItemKind::Addon, "ingest")
            .await
            .unwrap()
            .is_none()
    );

    // The period ends: the plan hold runs out and the workspace drops to Free.
    sqlx::query("UPDATE subscription_holds SET until = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let changes = billing::expire_holds(&pool).await.unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].1.after, "free");
    let entitlements = billing::entitlements(&pool, workspace_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entitlements.plan.code, "free");
    assert_eq!(entitlements.addon("ingest"), 1);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn ingest_is_hidden_until_it_exists(pool: PgPool) {
    let addons = billing::list_addons(&pool).await.unwrap();
    assert_eq!(addons.len(), 1);
    assert_eq!(addons[0].code, "ingest");
    assert_eq!(addons[0].status, billing::AddonVisibility::Hidden);
    assert!(billing::get_addon(&pool, "ingest").await.unwrap().is_some());
    assert!(billing::get_addon(&pool, "nope").await.unwrap().is_none());
}
