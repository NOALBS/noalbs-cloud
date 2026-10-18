//! Billing with Stripe: the price catalog (the `prices` table, managed from the
//! admin section), and turning Stripe subscriptions into what a workspace is
//! entitled to (see `db::billing`). The routes are in `routes/billing.rs`.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use db::billing::{ItemKind, ItemSync, SubscriptionSync};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{config::StripeConfig, stripe};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BillingInterval {
    Month,
    Year,
}

impl BillingInterval {
    pub const ALL: [Self; 2] = [Self::Month, Self::Year];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Month => "month",
            Self::Year => "year",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "month" => Some(Self::Month),
            "year" => Some(Self::Year),
            _ => None,
        }
    }
}

/// What a Stripe price sells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceEntry {
    pub kind: ItemKind,
    pub code: String,
    pub interval: BillingInterval,
}

/// The prices, both ways round: every price (archived ones too, so grandfathered
/// subscriptions still map to their plan or add-on), and the active one per
/// product and interval (what new subscriptions pay).
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    by_price: HashMap<String, PriceEntry>,
    active: HashMap<(ItemKind, String, BillingInterval), db::prices::Price>,
}

impl Catalog {
    pub fn from_prices(prices: Vec<db::prices::Price>) -> Self {
        let mut catalog = Self::default();
        for price in prices {
            let Some(interval) = BillingInterval::parse(&price.billing_interval) else {
                continue;
            };
            catalog.by_price.insert(
                price.stripe_price_id.clone(),
                PriceEntry {
                    kind: price.kind,
                    code: price.code.clone(),
                    interval,
                },
            );
            if price.active {
                catalog
                    .active
                    .insert((price.kind, price.code.clone(), interval), price);
            }
        }
        catalog
    }

    /// The catalog as it is in the database now (the table is small).
    pub async fn load(pool: &db::DbPool) -> Result<Self, db::Error> {
        Ok(Self::from_prices(db::prices::all(pool).await?))
    }

    pub fn lookup(&self, price_id: &str) -> Option<&PriceEntry> {
        self.by_price.get(price_id)
    }

    /// What new subscriptions pay for a plan or add-on at `interval`.
    pub fn active(
        &self,
        kind: ItemKind,
        code: &str,
        interval: BillingInterval,
    ) -> Option<&db::prices::Price> {
        self.active.get(&(kind, code.to_owned(), interval))
    }

    /// The Stripe price new subscriptions use, if it is sold that way.
    pub fn price(&self, kind: ItemKind, code: &str, interval: BillingInterval) -> Option<&str> {
        self.active(kind, code, interval)
            .map(|price| price.stripe_price_id.as_str())
    }

    pub fn plan_price(&self, code: &str, interval: BillingInterval) -> Option<&str> {
        self.price(ItemKind::Plan, code, interval)
    }
}

/// Billing is on: the Stripe client and the webhook secret.
pub struct Billing {
    pub stripe: stripe::Client,
    pub webhook_secret: String,
}

impl Billing {
    pub fn new(http: reqwest::Client, config: &StripeConfig) -> Self {
        Self {
            stripe: stripe::Client::new(http, &config.api_base, &config.secret_key),
            webhook_secret: config.webhook_secret.clone(),
        }
    }
}

fn timestamp(seconds: Option<i64>) -> Option<DateTime<Utc>> {
    seconds.and_then(|seconds| DateTime::from_timestamp(seconds, 0))
}

/// A Stripe subscription as `db::billing` stores it. Items with prices we don't
/// sell are left out (and logged), so they grant nothing.
pub fn to_sync(
    subscription: &stripe::Subscription,
    catalog: &Catalog,
    workspace_id: Uuid,
) -> SubscriptionSync {
    let mut interval = None;
    let items = subscription
        .items
        .data
        .iter()
        .filter_map(|item| {
            let Some(entry) = catalog.lookup(&item.price.id) else {
                tracing::warn!(
                    price = %item.price.id,
                    subscription = %subscription.id,
                    "Subscription item with a price that isn't in the config; ignoring it"
                );
                return None;
            };
            interval.get_or_insert(entry.interval);
            Some(ItemSync {
                stripe_item_id: item.id.clone(),
                stripe_price_id: item.price.id.clone(),
                kind: entry.kind,
                code: entry.code.clone(),
                quantity: i32::try_from(item.quantity.unwrap_or(1))
                    .unwrap_or(1)
                    .max(1),
            })
        })
        .collect();

    // Every item shares the subscription's period; newer API versions report it per item.
    let first = subscription.items.data.first();
    let interval = interval
        .or_else(|| {
            first
                .and_then(|item| item.price.recurring.as_ref())
                .and_then(|recurring| BillingInterval::parse(&recurring.interval))
        })
        .unwrap_or(BillingInterval::Month);

    SubscriptionSync {
        workspace_id,
        stripe_subscription_id: subscription.id.clone(),
        billing_interval: interval.as_str().to_owned(),
        status: subscription.status.clone(),
        current_period_start: timestamp(first.and_then(|item| item.current_period_start)),
        current_period_end: timestamp(first.and_then(|item| item.current_period_end)),
        cancel_at_period_end: subscription.cancel_at_period_end || subscription.cancel_at.is_some(),
        canceled_at: timestamp(subscription.canceled_at),
        items,
    }
}

/// How Stripe should charge for one step of a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proration {
    /// No charge or credit now; the next invoice bills the new quantity.
    None,
    /// Charge the rest of this period now (fails if the card is declined).
    ChargeNow,
}

/// What happens to the hold that keeps a lowered quantity until the period ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldChange {
    Keep,
    /// Keep at least this many until the period ends.
    Set(i32),
    Clear,
}

/// Moving one plan or add-on from what's `billed` (with `held` still paid for
/// this period) to `target`: the quantities to set on the Stripe item, in order,
/// and the hold afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantityChange {
    pub steps: Vec<(i32, Proration)>,
    pub hold: HoldChange,
}

/// Lowering applies at the end of the period (billed less from the next invoice,
/// held until then); raising is charged now, but only for what isn't already
/// paid for this period.
pub fn quantity_change(billed: i32, held: i32, target: i32) -> QuantityChange {
    let paid = billed.max(held);
    if target < billed {
        return QuantityChange {
            steps: vec![(target, Proration::None)],
            hold: HoldChange::Set(paid),
        };
    }
    let hold = if held > target {
        HoldChange::Keep
    } else if held > 0 {
        HoldChange::Clear
    } else {
        HoldChange::Keep
    };
    let steps = if target == billed {
        Vec::new()
    } else if target <= paid {
        vec![(target, Proration::None)]
    } else if paid > billed {
        vec![(paid, Proration::None), (target, Proration::ChargeNow)]
    } else {
        vec![(target, Proration::ChargeNow)]
    };
    QuantityChange { steps, hold }
}

#[cfg(test)]
mod tests {
    use db::billing::ItemKind;
    use uuid::Uuid;

    use super::{
        BillingInterval, Catalog, HoldChange, Proration, QuantityChange, quantity_change, to_sync,
    };
    fn price(
        kind: ItemKind,
        code: &str,
        interval: &str,
        id: &str,
        active: bool,
    ) -> db::prices::Price {
        db::prices::Price {
            id: Uuid::new_v4(),
            kind,
            code: code.into(),
            billing_interval: interval.into(),
            amount: 900,
            currency: "eur".into(),
            stripe_price_id: id.into(),
            active,
            created_at: chrono::Utc::now(),
        }
    }

    /// Pro monthly and yearly, Ingest monthly, and an old Pro monthly price.
    fn catalog() -> Catalog {
        Catalog::from_prices(vec![
            price(ItemKind::Plan, "pro", "month", "price_pro_m", true),
            price(ItemKind::Plan, "pro", "year", "price_pro_y", true),
            price(ItemKind::Addon, "ingest", "month", "price_ingest_m", true),
            price(ItemKind::Plan, "pro", "month", "price_pro_old", false),
        ])
    }

    fn change(steps: &[(i32, Proration)], hold: HoldChange) -> QuantityChange {
        QuantityChange {
            steps: steps.to_vec(),
            hold,
        }
    }

    #[test]
    fn raising_is_charged_now() {
        assert_eq!(
            quantity_change(1, 0, 3),
            change(&[(3, Proration::ChargeNow)], HoldChange::Keep)
        );
        // Buying something new (from 0).
        assert_eq!(
            quantity_change(0, 0, 1),
            change(&[(1, Proration::ChargeNow)], HoldChange::Keep)
        );
    }

    #[test]
    fn lowering_applies_at_the_end_of_the_period() {
        assert_eq!(
            quantity_change(3, 0, 1),
            change(&[(1, Proration::None)], HoldChange::Set(3))
        );
        // Lowering again keeps what was paid.
        assert_eq!(
            quantity_change(1, 3, 0),
            change(&[(0, Proration::None)], HoldChange::Set(3))
        );
    }

    #[test]
    fn taking_back_what_is_still_paid_for_costs_nothing() {
        // 3 → 1 earlier this period, now back to 2: already paid for.
        assert_eq!(
            quantity_change(1, 3, 2),
            change(&[(2, Proration::None)], HoldChange::Keep)
        );
        assert_eq!(
            quantity_change(1, 3, 3),
            change(&[(3, Proration::None)], HoldChange::Clear)
        );
        // Beyond it: the paid part free, the rest charged.
        assert_eq!(
            quantity_change(1, 3, 5),
            change(
                &[(3, Proration::None), (5, Proration::ChargeNow)],
                HoldChange::Clear
            )
        );
        // Keeping Pro after choosing Free this period.
        assert_eq!(
            quantity_change(0, 1, 1),
            change(&[(1, Proration::None)], HoldChange::Clear)
        );
    }

    #[test]
    fn no_change_does_nothing() {
        assert_eq!(quantity_change(2, 0, 2), change(&[], HoldChange::Keep));
        assert_eq!(quantity_change(2, 3, 2), change(&[], HoldChange::Keep));
    }

    #[test]
    fn the_catalog_maps_prices_both_ways() {
        let catalog = catalog();
        let entry = catalog.lookup("price_pro_y").unwrap();
        assert_eq!(
            (entry.kind, entry.code.as_str(), entry.interval),
            (ItemKind::Plan, "pro", BillingInterval::Year)
        );
        assert_eq!(
            catalog.lookup("price_ingest_m").unwrap().kind,
            ItemKind::Addon
        );
        assert_eq!(
            catalog.plan_price("pro", BillingInterval::Month),
            Some("price_pro_m")
        );
        assert_eq!(catalog.plan_price("ingest", BillingInterval::Month), None);
        assert_eq!(
            catalog.plan_price("pro", BillingInterval::Year),
            Some("price_pro_y")
        );
        // An archived price still maps its old subscriptions to Pro, but isn't sold.
        assert_eq!(catalog.lookup("price_pro_old").unwrap().code, "pro");
        assert_eq!(
            catalog
                .active(ItemKind::Plan, "pro", BillingInterval::Month)
                .unwrap()
                .stripe_price_id,
            "price_pro_m"
        );
        assert!(catalog.lookup("price_other").is_none());
    }

    #[test]
    fn a_stripe_subscription_becomes_a_sync() {
        let subscription: crate::stripe::Subscription = serde_json::from_value(serde_json::json!({
            "id": "sub_1",
            "object": "subscription",
            "customer": "cus_1",
            "status": "active",
            "cancel_at_period_end": false,
            "cancel_at": 1_792_000_000,
            "canceled_at": null,
            "metadata": { "workspace_id": "00000000-0000-0000-0000-000000000001" },
            "items": { "object": "list", "has_more": false, "data": [
                { "id": "si_1", "quantity": 1, "current_period_start": 1_790_000_000,
                  "current_period_end": 1_792_000_000,
                  "price": { "id": "price_pro_y", "unit_amount": 9000, "currency": "eur",
                             "recurring": { "interval": "year" } } },
                { "id": "si_2", "quantity": 2,
                  "current_period_start": 1_790_000_000, "current_period_end": 1_792_000_000,
                  "price": { "id": "price_ingest_m", "unit_amount": 500, "currency": "eur",
                             "recurring": { "interval": "month" } } },
                { "id": "si_3", "quantity": 1,
                  "price": { "id": "price_unknown", "unit_amount": 1, "currency": "eur", "recurring": null } }
            ] }
        }))
        .unwrap();

        let workspace_id = Uuid::new_v4();
        let sync = to_sync(&subscription, &catalog(), workspace_id);
        assert_eq!(sync.workspace_id, workspace_id);
        assert_eq!(sync.billing_interval, "year");
        assert_eq!(sync.status, "active");
        // Scheduled to cancel (the portal sets `cancel_at`).
        assert!(sync.cancel_at_period_end);
        assert_eq!(
            sync.current_period_end.map(|at| at.timestamp()),
            Some(1_792_000_000)
        );
        assert_eq!(
            sync.items
                .iter()
                .map(|item| (item.kind, item.code.as_str(), item.quantity))
                .collect::<Vec<_>>(),
            [(ItemKind::Plan, "pro", 1), (ItemKind::Addon, "ingest", 2)]
        );
    }
}
