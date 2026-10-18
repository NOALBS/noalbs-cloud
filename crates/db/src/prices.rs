//! What plans and add-ons cost: one Stripe price per product and billing
//! interval, set from the admin section. Changing a price archives the old one;
//! subscriptions on it keep it (grandfathered) and still map to their product.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{billing::ItemKind, error::Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Price {
    pub id: Uuid,
    pub kind: ItemKind,
    pub code: String,
    /// `month` or `year`.
    pub billing_interval: String,
    /// In the currency's smallest unit; per unit for add-ons.
    pub amount: i32,
    pub currency: String,
    pub stripe_price_id: String,
    /// Sold to new subscriptions.
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    kind: String,
    code: String,
    billing_interval: String,
    amount: i32,
    currency: String,
    stripe_price_id: String,
    active: bool,
    created_at: DateTime<Utc>,
}

impl Row {
    fn into_price(self) -> Option<Price> {
        Some(Price {
            id: self.id,
            kind: ItemKind::parse(&self.kind)?,
            code: self.code,
            billing_interval: self.billing_interval,
            amount: self.amount,
            currency: self.currency,
            stripe_price_id: self.stripe_price_id,
            active: self.active,
            created_at: self.created_at,
        })
    }
}

macro_rules! price_columns {
    () => {
        "id, kind, code, billing_interval, amount, currency, stripe_price_id, active, created_at"
    };
}

/// Every price, archived ones included (for recognising old subscriptions),
/// newest first.
#[tracing::instrument(skip(pool))]
pub async fn all(pool: &PgPool) -> Result<Vec<Price>> {
    let rows = sqlx::query_as::<_, Row>(concat!(
        "SELECT ",
        price_columns!(),
        " FROM prices ORDER BY created_at DESC"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().filter_map(Row::into_price).collect())
}

/// The price new subscriptions pay for a product at `interval`.
#[tracing::instrument(skip(pool))]
pub async fn active(
    pool: &PgPool,
    kind: ItemKind,
    code: &str,
    interval: &str,
) -> Result<Option<Price>> {
    let row = sqlx::query_as::<_, Row>(concat!(
        "SELECT ",
        price_columns!(),
        " FROM prices WHERE kind = $1 AND code = $2 AND billing_interval = $3 AND active"
    ))
    .bind(kind.as_str())
    .bind(code)
    .bind(interval)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(Row::into_price))
}

/// New subscriptions pay this from now on. Returns the price it replaces (now
/// archived), whose subscriptions keep it.
#[tracing::instrument(skip(pool))]
pub async fn set_active(
    pool: &PgPool,
    kind: ItemKind,
    code: &str,
    interval: &str,
    amount: i32,
    currency: &str,
    stripe_price_id: &str,
) -> Result<Option<Price>> {
    let mut tx = pool.begin().await?;
    let replaced = sqlx::query_as::<_, Row>(concat!(
        "UPDATE prices SET active = false
         WHERE kind = $1 AND code = $2 AND billing_interval = $3 AND active
         RETURNING ",
        price_columns!()
    ))
    .bind(kind.as_str())
    .bind(code)
    .bind(interval)
    .fetch_optional(&mut *tx)
    .await?;
    sqlx::query(
        "
        INSERT INTO prices (kind, code, billing_interval, amount, currency, stripe_price_id)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (stripe_price_id) DO UPDATE SET active = true
        ",
    )
    .bind(kind.as_str())
    .bind(code)
    .bind(interval)
    .bind(amount)
    .bind(currency)
    .bind(stripe_price_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(replaced
        .and_then(Row::into_price)
        .filter(|price| price.stripe_price_id != stripe_price_id))
}

/// Stops selling a product at `interval`. Returns the archived price.
#[tracing::instrument(skip(pool))]
pub async fn archive(
    pool: &PgPool,
    kind: ItemKind,
    code: &str,
    interval: &str,
) -> Result<Option<Price>> {
    let row = sqlx::query_as::<_, Row>(concat!(
        "UPDATE prices SET active = false
         WHERE kind = $1 AND code = $2 AND billing_interval = $3 AND active
         RETURNING ",
        price_columns!()
    ))
    .bind(kind.as_str())
    .bind(code)
    .bind(interval)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(Row::into_price))
}

/// The currencies active prices use. Checkout can't mix currencies in one
/// subscription, so there should be at most one.
#[tracing::instrument(skip(pool))]
pub async fn currencies_in_use(pool: &PgPool) -> Result<Vec<String>> {
    let currencies =
        sqlx::query_scalar::<_, String>("SELECT DISTINCT currency FROM prices WHERE active")
            .fetch_all(pool)
            .await?;
    Ok(currencies)
}
