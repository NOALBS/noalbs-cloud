//! Billing: workspaces' Stripe customers and the subscriptions the API's Stripe
//! webhook mirrors. A subscription's items are what it pays for (the base plan
//! and any add-ons); `workspaces.plan_code` is derived from the live one.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{error::Result, plans::Plan};

/// Statuses that entitle a workspace to what it pays for. `past_due` counts:
/// Stripe is still retrying the payment (the grace period).
pub const LIVE_STATUSES: [&str; 3] = ["trialing", "active", "past_due"];

/// What a subscription item pays for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemKind {
    /// A base plan; the item's code is a plan code.
    Plan,
    /// An add-on, bought on top of (or without) a plan.
    Addon,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Addon => "addon",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "plan" => Some(Self::Plan),
            "addon" => Some(Self::Addon),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ItemSync {
    pub stripe_item_id: String,
    pub stripe_price_id: String,
    pub kind: ItemKind,
    pub code: String,
    pub quantity: i32,
}

/// A subscription as Stripe reports it, ready to store.
#[derive(Debug, Clone, PartialEq)]
pub struct SubscriptionSync {
    pub workspace_id: Uuid,
    pub stripe_subscription_id: String,
    /// `month` or `year`.
    pub billing_interval: String,
    pub status: String,
    pub current_period_start: Option<DateTime<Utc>>,
    pub current_period_end: Option<DateTime<Utc>>,
    pub cancel_at_period_end: bool,
    pub canceled_at: Option<DateTime<Utc>>,
    pub items: Vec<ItemSync>,
}

/// A stored subscription, with the plan it pays for (if any).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Subscription {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub stripe_subscription_id: String,
    pub billing_interval: String,
    pub status: String,
    pub current_period_start: Option<DateTime<Utc>>,
    pub current_period_end: Option<DateTime<Utc>>,
    pub cancel_at_period_end: bool,
    pub canceled_at: Option<DateTime<Utc>>,
    /// The plan it bills for.
    pub plan_code: Option<String>,
    /// A plan no longer billed but paid until `held_plan_until` (a downgrade
    /// that applies at the end of the period).
    pub held_plan_code: Option<String>,
    pub held_plan_until: Option<DateTime<Utc>>,
}

/// The workspace's plan before and after a sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanChange {
    pub before: String,
    pub after: String,
}

impl PlanChange {
    pub fn changed(&self) -> bool {
        self.before != self.after
    }
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Addon {
    pub code: String,
    pub quantity: i32,
}

/// What a workspace may use: its plan's limits plus any add-ons. Features ask
/// this rather than reading `plan_code`.
#[derive(Debug, Clone)]
pub struct Entitlements {
    pub plan: Plan,
    pub addons: Vec<Addon>,
}

impl Entitlements {
    /// How many of an add-on the workspace has (0 without it).
    pub fn addon(&self, code: &str) -> i32 {
        self.addons
            .iter()
            .filter(|addon| addon.code == code)
            .map(|addon| addon.quantity)
            .sum()
    }
}

/// Stores `customer_id` as the workspace's Stripe customer unless it already
/// has one, and returns the one it ends up with (so racing checkouts share a
/// customer). `None` if the workspace doesn't exist.
#[tracing::instrument(skip(pool))]
pub async fn set_customer_if_missing(
    pool: &PgPool,
    workspace_id: Uuid,
    customer_id: &str,
) -> Result<Option<String>> {
    let customer = sqlx::query_scalar::<_, Option<String>>(
        "
        WITH updated AS (
            UPDATE workspaces SET stripe_customer_id = $2
            WHERE id = $1 AND stripe_customer_id IS NULL
            RETURNING stripe_customer_id
        )
        SELECT stripe_customer_id FROM updated
        UNION ALL
        SELECT stripe_customer_id FROM workspaces WHERE id = $1 AND NOT EXISTS (SELECT 1 FROM updated)
        ",
    )
    .bind(workspace_id)
    .bind(customer_id)
    .fetch_optional(pool)
    .await?;

    Ok(customer.flatten())
}

#[tracing::instrument(skip(pool))]
pub async fn workspace_by_customer(pool: &PgPool, customer_id: &str) -> Result<Option<Uuid>> {
    let workspace_id =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM workspaces WHERE stripe_customer_id = $1")
            .bind(customer_id)
            .fetch_optional(pool)
            .await?;
    Ok(workspace_id)
}

/// Stores a subscription and its items as Stripe reports them, then derives
/// the workspace's plan from its live subscription (and any hold). `None` if
/// the workspace doesn't exist.
#[tracing::instrument(skip(pool, sync), fields(subscription = %sync.stripe_subscription_id))]
pub async fn apply_subscription(
    pool: &PgPool,
    sync: &SubscriptionSync,
) -> Result<Option<PlanChange>> {
    let mut tx = pool.begin().await?;

    let Some(before) = sqlx::query_scalar::<_, String>(
        "SELECT plan_code FROM workspaces WHERE id = $1 FOR UPDATE",
    )
    .bind(sync.workspace_id)
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };

    let subscription_id = sqlx::query_scalar::<_, Uuid>(
        "
        INSERT INTO subscriptions (
            workspace_id, stripe_subscription_id, billing_interval, status,
            current_period_start, current_period_end, cancel_at_period_end, canceled_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (stripe_subscription_id) DO UPDATE SET
            billing_interval = EXCLUDED.billing_interval,
            status = EXCLUDED.status,
            current_period_start = EXCLUDED.current_period_start,
            current_period_end = EXCLUDED.current_period_end,
            cancel_at_period_end = EXCLUDED.cancel_at_period_end,
            canceled_at = EXCLUDED.canceled_at
        RETURNING id
        ",
    )
    .bind(sync.workspace_id)
    .bind(&sync.stripe_subscription_id)
    .bind(&sync.billing_interval)
    .bind(&sync.status)
    .bind(sync.current_period_start)
    .bind(sync.current_period_end)
    .bind(sync.cancel_at_period_end)
    .bind(sync.canceled_at)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM subscription_items WHERE subscription_id = $1")
        .bind(subscription_id)
        .execute(&mut *tx)
        .await?;
    for item in &sync.items {
        sqlx::query(
            "
            INSERT INTO subscription_items
                (subscription_id, stripe_item_id, stripe_price_id, kind, code, quantity)
            VALUES ($1, $2, $3, $4, $5, $6)
            ",
        )
        .bind(subscription_id)
        .bind(&item.stripe_item_id)
        .bind(&item.stripe_price_id)
        .bind(item.kind.as_str())
        .bind(&item.code)
        .bind(item.quantity)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("DELETE FROM subscription_holds WHERE subscription_id = $1 AND until <= now()")
        .bind(subscription_id)
        .execute(&mut *tx)
        .await?;

    let after = set_plan_from_billing(&mut tx, sync.workspace_id, &before).await?;
    tx.commit().await?;
    Ok(Some(PlanChange { before, after }))
}

/// Derives `plan_code` from the live subscription: its plan item, else a plan
/// still held until the end of the period, else `free`. Returns the new plan.
async fn set_plan_from_billing(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    workspace_id: Uuid,
    before: &str,
) -> Result<String> {
    let after = sqlx::query_scalar::<_, String>(
        "
        SELECT COALESCE(
            (SELECT plans.code
             FROM subscriptions
             JOIN subscription_items ON subscription_items.subscription_id = subscriptions.id
                 AND subscription_items.kind = 'plan'
             JOIN plans ON plans.code = subscription_items.code
             WHERE subscriptions.workspace_id = $1 AND subscriptions.status = ANY($2)
             LIMIT 1),
            (SELECT plans.code
             FROM subscriptions
             JOIN subscription_holds ON subscription_holds.subscription_id = subscriptions.id
                 AND subscription_holds.kind = 'plan' AND subscription_holds.until > now()
             JOIN plans ON plans.code = subscription_holds.code
             WHERE subscriptions.workspace_id = $1 AND subscriptions.status = ANY($2)
             LIMIT 1),
            'free'
        )
        ",
    )
    .bind(workspace_id)
    .bind(&LIVE_STATUSES[..])
    .fetch_one(&mut **tx)
    .await?;

    if after != before {
        sqlx::query("UPDATE workspaces SET plan_code = $2 WHERE id = $1")
            .bind(workspace_id)
            .bind(&after)
            .execute(&mut **tx)
            .await?;
    }
    Ok(after)
}

/// The subscription that currently entitles the workspace, if any.
#[tracing::instrument(skip(pool))]
pub async fn live_subscription(pool: &PgPool, workspace_id: Uuid) -> Result<Option<Subscription>> {
    let subscription = sqlx::query_as::<_, Subscription>(
        "
        SELECT subscriptions.id, subscriptions.workspace_id, subscriptions.stripe_subscription_id,
               subscriptions.billing_interval, subscriptions.status,
               subscriptions.current_period_start, subscriptions.current_period_end,
               subscriptions.cancel_at_period_end, subscriptions.canceled_at,
               subscription_items.code AS plan_code,
               plan_hold.code AS held_plan_code, plan_hold.until AS held_plan_until
        FROM subscriptions
        LEFT JOIN subscription_items ON subscription_items.subscription_id = subscriptions.id
            AND subscription_items.kind = 'plan'
        LEFT JOIN subscription_holds plan_hold ON plan_hold.subscription_id = subscriptions.id
            AND plan_hold.kind = 'plan' AND plan_hold.until > now()
        WHERE subscriptions.workspace_id = $1 AND subscriptions.status = ANY($2)
        LIMIT 1
        ",
    )
    .bind(workspace_id)
    .bind(&LIVE_STATUSES[..])
    .fetch_optional(pool)
    .await?;
    Ok(subscription)
}

/// What a subscription is billed for right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub stripe_item_id: String,
    pub stripe_price_id: String,
    pub kind: ItemKind,
    pub code: String,
    pub quantity: i32,
}

#[tracing::instrument(skip(pool))]
pub async fn items(pool: &PgPool, subscription_id: Uuid) -> Result<Vec<Item>> {
    let rows = sqlx::query_as::<_, (String, String, String, String, i32)>(
        "
        SELECT stripe_item_id, stripe_price_id, kind, code, quantity
        FROM subscription_items
        WHERE subscription_id = $1
        ORDER BY kind DESC, code
        ",
    )
    .bind(subscription_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|(stripe_item_id, stripe_price_id, kind, code, quantity)| {
            Some(Item {
                stripe_item_id,
                stripe_price_id,
                kind: ItemKind::parse(&kind)?,
                code,
                quantity,
            })
        })
        .collect())
}

/// Paid for until `until`, though no longer billed.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Hold {
    pub quantity: i32,
    pub until: DateTime<Utc>,
}

/// The hold on something the subscription no longer bills (fully), if it
/// hasn't run out.
#[tracing::instrument(skip(pool))]
pub async fn hold(
    pool: &PgPool,
    subscription_id: Uuid,
    kind: ItemKind,
    code: &str,
) -> Result<Option<Hold>> {
    let hold = sqlx::query_as::<_, Hold>(
        "
        SELECT quantity, until FROM subscription_holds
        WHERE subscription_id = $1 AND kind = $2 AND code = $3 AND until > now()
        ",
    )
    .bind(subscription_id)
    .bind(kind.as_str())
    .bind(code)
    .fetch_optional(pool)
    .await?;
    Ok(hold)
}

/// Keeps `quantity` of something until `until` (the end of the paid period).
/// Lowering twice in one period keeps the higher amount.
#[tracing::instrument(skip(pool))]
pub async fn set_hold(
    pool: &PgPool,
    subscription_id: Uuid,
    kind: ItemKind,
    code: &str,
    quantity: i32,
    until: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "
        INSERT INTO subscription_holds (subscription_id, kind, code, quantity, until)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (subscription_id, kind, code) DO UPDATE SET
            quantity = CASE WHEN subscription_holds.until > now()
                            THEN GREATEST(subscription_holds.quantity, EXCLUDED.quantity)
                            ELSE EXCLUDED.quantity END,
            until = EXCLUDED.until
        ",
    )
    .bind(subscription_id)
    .bind(kind.as_str())
    .bind(code)
    .bind(quantity)
    .bind(until)
    .execute(pool)
    .await?;
    Ok(())
}

#[tracing::instrument(skip(pool))]
pub async fn clear_hold(
    pool: &PgPool,
    subscription_id: Uuid,
    kind: ItemKind,
    code: &str,
) -> Result<()> {
    sqlx::query(
        "DELETE FROM subscription_holds WHERE subscription_id = $1 AND kind = $2 AND code = $3",
    )
    .bind(subscription_id)
    .bind(kind.as_str())
    .bind(code)
    .execute(pool)
    .await?;
    Ok(())
}

/// Deletes holds that ran out and re-derives the plan of their workspaces.
/// Returns the workspaces whose plan changed. (Stripe's renewal webhook does
/// the same; this catches a missed one.)
#[tracing::instrument(skip(pool))]
pub async fn expire_holds(pool: &PgPool) -> Result<Vec<(Uuid, PlanChange)>> {
    let workspaces = sqlx::query_scalar::<_, Uuid>(
        "
        WITH expired AS (
            DELETE FROM subscription_holds WHERE until <= now() RETURNING subscription_id
        )
        SELECT DISTINCT subscriptions.workspace_id
        FROM expired JOIN subscriptions ON subscriptions.id = expired.subscription_id
        ",
    )
    .fetch_all(pool)
    .await?;

    let mut changes = Vec::new();
    for workspace_id in workspaces {
        let mut tx = pool.begin().await?;
        let Some(before) = sqlx::query_scalar::<_, String>(
            "SELECT plan_code FROM workspaces WHERE id = $1 FOR UPDATE",
        )
        .bind(workspace_id)
        .fetch_optional(&mut *tx)
        .await?
        else {
            continue;
        };
        let after = set_plan_from_billing(&mut tx, workspace_id, &before).await?;
        tx.commit().await?;
        let change = PlanChange { before, after };
        if change.changed() {
            changes.push((workspace_id, change));
        }
    }
    Ok(changes)
}

/// An add-on as a workspace has it: what's billed, and what's still held
/// until the end of the period after lowering it.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct AddonStatus {
    pub code: String,
    pub billed: i32,
    pub held: Option<i32>,
    pub held_until: Option<DateTime<Utc>>,
}

impl AddonStatus {
    /// How many the workspace may use now.
    pub fn quantity(&self) -> i32 {
        self.billed.max(self.held.unwrap_or(0))
    }
}

/// The workspace's add-ons (billed or held) on its live subscription.
#[tracing::instrument(skip(pool))]
pub async fn addon_statuses(pool: &PgPool, workspace_id: Uuid) -> Result<Vec<AddonStatus>> {
    let statuses = sqlx::query_as::<_, AddonStatus>(
        "
        WITH live AS (
            SELECT id FROM subscriptions
            WHERE workspace_id = $1 AND status = ANY($2)
        ),
        billed AS (
            SELECT code, sum(quantity)::int4 AS quantity FROM subscription_items
            WHERE subscription_id IN (SELECT id FROM live) AND kind = 'addon'
            GROUP BY code
        ),
        held AS (
            SELECT code, max(quantity) AS quantity, max(until) AS until FROM subscription_holds
            WHERE subscription_id IN (SELECT id FROM live) AND kind = 'addon' AND until > now()
            GROUP BY code
        )
        SELECT COALESCE(billed.code, held.code) AS code,
               COALESCE(billed.quantity, 0) AS billed,
               held.quantity AS held,
               held.until AS held_until
        FROM billed FULL JOIN held ON held.code = billed.code
        ORDER BY 1
        ",
    )
    .bind(workspace_id)
    .bind(&LIVE_STATUSES[..])
    .fetch_all(pool)
    .await?;
    Ok(statuses)
}

/// What the workspace may use. `None` if it doesn't exist.
#[tracing::instrument(skip(pool))]
pub async fn entitlements(pool: &PgPool, workspace_id: Uuid) -> Result<Option<Entitlements>> {
    let Some(plan) = crate::plans::for_workspace(pool, workspace_id).await? else {
        return Ok(None);
    };
    let addons = addon_statuses(pool, workspace_id)
        .await?
        .into_iter()
        .map(|status| Addon {
            quantity: status.quantity(),
            code: status.code,
        })
        .filter(|addon| addon.quantity > 0)
        .collect();

    Ok(Some(Entitlements { plan, addons }))
}

/// Whether an add-on shows on the Plan page and can be bought.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum AddonVisibility {
    /// Not listed, except to workspaces that already have it (so they can lower it).
    Hidden,
    /// Listed, not for sale yet.
    ComingSoon,
    /// Listed, sold before but paused (e.g. at capacity): nobody can buy more,
    /// those who have it keep it.
    Unavailable,
    /// For sale, once it has a price.
    Available,
}

/// An add-on as sold: the catalog entry.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AddonProduct {
    pub code: String,
    pub name: String,
    pub description: String,
    pub unit: String,
    pub max_quantity: i32,
    pub status: AddonVisibility,
    pub position: i32,
    pub stripe_product_id: Option<String>,
}

macro_rules! addon_columns {
    () => {
        "code, name, description, unit, max_quantity, status, position, stripe_product_id"
    };
}

/// What the admin section sets on an add-on (the code is fixed once created).
#[derive(Debug, Clone)]
pub struct AddonFields<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub unit: &'a str,
    pub max_quantity: i32,
    pub status: AddonVisibility,
    pub position: i32,
}

#[tracing::instrument(skip(pool))]
pub async fn list_addons(pool: &PgPool) -> Result<Vec<AddonProduct>> {
    let addons = sqlx::query_as::<_, AddonProduct>(concat!(
        "SELECT ",
        addon_columns!(),
        " FROM addons ORDER BY position, name"
    ))
    .fetch_all(pool)
    .await?;
    Ok(addons)
}

#[tracing::instrument(skip(pool))]
pub async fn get_addon(pool: &PgPool, code: &str) -> Result<Option<AddonProduct>> {
    let addon = sqlx::query_as::<_, AddonProduct>(concat!(
        "SELECT ",
        addon_columns!(),
        " FROM addons WHERE code = $1"
    ))
    .bind(code)
    .fetch_optional(pool)
    .await?;
    Ok(addon)
}

/// Creates an add-on. A taken code is a unique violation.
#[tracing::instrument(skip(pool, fields))]
pub async fn create_addon(
    pool: &PgPool,
    code: &str,
    fields: &AddonFields<'_>,
) -> Result<AddonProduct> {
    let addon = sqlx::query_as::<_, AddonProduct>(concat!(
        "
        INSERT INTO addons (code, name, description, unit, max_quantity, status, position)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING ",
        addon_columns!()
    ))
    .bind(code)
    .bind(fields.name)
    .bind(fields.description)
    .bind(fields.unit)
    .bind(fields.max_quantity)
    .bind(fields.status)
    .bind(fields.position)
    .fetch_one(pool)
    .await?;
    Ok(addon)
}

#[tracing::instrument(skip(pool, fields))]
pub async fn update_addon(
    pool: &PgPool,
    code: &str,
    fields: &AddonFields<'_>,
) -> Result<Option<AddonProduct>> {
    let addon = sqlx::query_as::<_, AddonProduct>(concat!(
        "
        UPDATE addons SET
            name = $2, description = $3, unit = $4, max_quantity = $5, status = $6, position = $7
        WHERE code = $1
        RETURNING ",
        addon_columns!()
    ))
    .bind(code)
    .bind(fields.name)
    .bind(fields.description)
    .bind(fields.unit)
    .bind(fields.max_quantity)
    .bind(fields.status)
    .bind(fields.position)
    .fetch_optional(pool)
    .await?;
    Ok(addon)
}

#[tracing::instrument(skip(pool))]
pub async fn set_addon_stripe_product(pool: &PgPool, code: &str, product_id: &str) -> Result<()> {
    sqlx::query("UPDATE addons SET stripe_product_id = $2 WHERE code = $1")
        .bind(code)
        .bind(product_id)
        .execute(pool)
        .await?;
    Ok(())
}
