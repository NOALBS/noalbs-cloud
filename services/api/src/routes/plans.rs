use axum::{Json, extract::State};
use db::{billing::ItemKind, workspaces::WorkspacePermission};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    billing::{BillingInterval, Catalog},
    permissions::{AccountView, CurrentUser, Via, WorkspaceAuthorized},
    routes::{ApiError, ApiResult, ErrorBody, billing::payment_method_missing},
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_plans))
        .routes(routes!(list_addons))
        .routes(routes!(workspace_plan))
}

/// A plan and the limits it gives a workspace.
#[derive(Debug, Serialize, ToSchema)]
pub struct PlanDto {
    pub code: String,
    pub name: String,
    /// Left out (`null`) for a customer of a client workspace without an allocation of
    /// its own: its limit is its organization's pool, which isn't theirs to see.
    pub max_switchers: Option<i32>,
    /// Per switcher.
    pub max_stream_servers: i32,
    pub max_members: i32,
    /// Days of stream history kept.
    pub history_days: i32,
    /// Workspaces on it may connect their own chat bot.
    pub custom_chat_bot: bool,
    /// What it costs, per billing period it's sold with. Empty for free plans
    /// and when billing is off.
    pub prices: Vec<PlanPriceDto>,
}

impl From<db::plans::Plan> for PlanDto {
    fn from(plan: db::plans::Plan) -> Self {
        Self {
            code: plan.code,
            name: plan.name,
            max_switchers: Some(plan.max_switchers),
            max_stream_servers: plan.max_stream_servers,
            max_members: plan.max_workspace_members,
            history_days: plan.history_days,
            custom_chat_bot: plan.custom_chat_bot,
            prices: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlanPriceDto {
    pub interval: BillingInterval,
    /// In the currency's smallest unit (e.g. cents).
    pub amount: i64,
    /// ISO code, lowercase (e.g. `eur`).
    pub currency: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlanUsage {
    pub switchers: i64,
    pub members: i64,
}

/// The workspace's Stripe subscription, while it has one that counts.
#[derive(Debug, Serialize, ToSchema)]
pub struct SubscriptionDto {
    /// `active`, `trialing` or `past_due` (a payment failed and Stripe is retrying).
    pub status: String,
    pub plan_code: Option<String>,
    pub interval: BillingInterval,
    pub current_period_start: Option<chrono::DateTime<chrono::Utc>>,
    /// When the current period ends: the next renewal, or when it stops.
    pub current_period_end: Option<chrono::DateTime<chrono::Utc>>,
    /// Canceled, but paid up until `current_period_end`.
    pub cancel_at_period_end: bool,
    /// A plan no longer billed but kept until `held_plan_until` (moving to Free
    /// at the end of the period).
    pub held_plan_code: Option<String>,
    pub held_plan_until: Option<chrono::DateTime<chrono::Utc>>,
}

/// An add-on the workspace has.
#[derive(Debug, Serialize, ToSchema)]
pub struct WorkspaceAddonDto {
    pub code: String,
    /// How many it may use now.
    pub quantity: i32,
    /// How many are billed from the next invoice on (less than `quantity` after
    /// lowering it, until `held_until`).
    pub billed: i32,
    pub held_until: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct WorkspacePlanDto {
    pub plan: PlanDto,
    pub usage: PlanUsage,
    /// Whether plans can be bought here (Stripe is configured).
    pub billing_enabled: bool,
    /// Whether you may subscribe or manage billing (`billing.manage`).
    pub can_manage_billing: bool,
    pub subscription: Option<SubscriptionDto>,
    /// The subscription will renew but there's no card: an ownership transfer
    /// removed the previous owner's and the new owner hasn't added one yet.
    pub needs_payment_method: bool,
    pub addons: Vec<WorkspaceAddonDto>,
}

pub async fn workspace_plan_dto(
    state: &ApiState,
    workspace_id: Uuid,
) -> ApiResult<WorkspacePlanDto> {
    let plan = db::plans::for_workspace(&state.db, workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Workspace not found"))?;
    let usage = PlanUsage {
        switchers: db::switchers::count_in_workspace(&state.db, workspace_id).await?,
        members: db::workspaces::member_count(&state.db, workspace_id).await?,
    };
    let subscription = db::billing::live_subscription(&state.db, workspace_id)
        .await?
        .map(|subscription| SubscriptionDto {
            status: subscription.status,
            plan_code: subscription.plan_code,
            interval: BillingInterval::parse(&subscription.billing_interval)
                .unwrap_or(BillingInterval::Month),
            current_period_start: subscription.current_period_start,
            current_period_end: subscription.current_period_end,
            cancel_at_period_end: subscription.cancel_at_period_end,
            held_plan_code: subscription.held_plan_code,
            held_plan_until: subscription.held_plan_until,
        });
    let addons = db::billing::addon_statuses(&state.db, workspace_id)
        .await?
        .into_iter()
        .map(|status| WorkspaceAddonDto {
            quantity: status.quantity(),
            billed: status.billed,
            held_until: status.held.and(status.held_until),
            code: status.code,
        })
        .collect();

    let card_removed = db::workspaces::get_by_id(&state.db, workspace_id)
        .await?
        .is_some_and(|workspace| workspace.needs_payment_method);
    let needs_payment_method = card_removed
        && subscription
            .as_ref()
            .is_some_and(|subscription| !subscription.cancel_at_period_end);

    Ok(WorkspacePlanDto {
        plan: plan.into(),
        usage,
        billing_enabled: state.billing.is_some(),
        can_manage_billing: false,
        subscription,
        needs_payment_method,
        addons,
    })
}

pub fn member_limit_error() -> ApiError {
    ApiError::forbidden(
        "plan_limit",
        "This workspace has reached its member limit. Upgrade the plan to add more people.",
    )
}

/// Fails with `plan_limit` when the workspace has no room for another member.
pub async fn ensure_member_room(state: &ApiState, workspace_id: Uuid) -> ApiResult<()> {
    let WorkspacePlanDto { plan, usage, .. } = workspace_plan_dto(state, workspace_id).await?;

    if usage.members >= i64::from(plan.max_members) {
        return Err(member_limit_error());
    }

    Ok(())
}

/// List the plans a workspace can be on, with their prices.
#[utoipa::path(get, path = "/plans", tag = "plans",
    responses((status = OK, body = Vec<PlanDto>), (status = UNAUTHORIZED, body = ErrorBody)))]
async fn list_plans(
    State(state): State<ApiState>,
    _current_user: CurrentUser,
) -> ApiResult<Json<Vec<PlanDto>>> {
    let plans = db::plans::list_active(&state.db).await?;
    let mut dtos: Vec<PlanDto> = plans.into_iter().map(Into::into).collect();
    let catalog = sold(&state).await?;
    for dto in &mut dtos {
        dto.prices = prices(catalog.as_ref(), ItemKind::Plan, &dto.code);
    }

    Ok(Json(dtos))
}

/// The catalog, while billing is on (without it nothing can be bought, so no
/// prices are shown).
async fn sold(state: &ApiState) -> ApiResult<Option<Catalog>> {
    if state.billing.is_none() {
        return Ok(None);
    }
    Ok(Some(Catalog::load(&state.db).await?))
}

/// What a plan or add-on costs new subscribers per billing period.
pub fn prices(catalog: Option<&Catalog>, kind: ItemKind, code: &str) -> Vec<PlanPriceDto> {
    let Some(catalog) = catalog else {
        return Vec::new();
    };
    BillingInterval::ALL
        .into_iter()
        .filter_map(|interval| {
            let price = catalog.active(kind, code, interval)?;
            Some(PlanPriceDto {
                interval,
                amount: i64::from(price.amount),
                currency: price.currency.clone(),
            })
        })
        .collect()
}

/// An add-on that can be bought per unit, on top of (or without) a plan.
#[derive(Debug, Serialize, ToSchema)]
pub struct AddonDto {
    pub code: String,
    pub name: String,
    pub description: String,
    /// What one unit is called, e.g. `slot`.
    pub unit: String,
    pub max_quantity: i32,
    /// `available` only when it can be bought (it has a price); a hidden one is
    /// listed so workspaces that have it can still lower it.
    pub status: AddonVisibility,
    /// Per unit.
    pub prices: Vec<PlanPriceDto>,
}

/// Whether an add-on shows on the Plan page and can be bought.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AddonVisibility {
    /// Not listed (except to workspaces that already have it).
    Hidden,
    /// Listed, not for sale yet.
    ComingSoon,
    /// Listed, sold before but paused: nobody can buy more, those who have it keep it.
    Unavailable,
    Available,
}

impl From<db::billing::AddonVisibility> for AddonVisibility {
    fn from(status: db::billing::AddonVisibility) -> Self {
        use db::billing::AddonVisibility as Db;
        match status {
            Db::Hidden => Self::Hidden,
            Db::ComingSoon => Self::ComingSoon,
            Db::Unavailable => Self::Unavailable,
            Db::Available => Self::Available,
        }
    }
}

impl From<AddonVisibility> for db::billing::AddonVisibility {
    fn from(status: AddonVisibility) -> Self {
        match status {
            AddonVisibility::Hidden => Self::Hidden,
            AddonVisibility::ComingSoon => Self::ComingSoon,
            AddonVisibility::Unavailable => Self::Unavailable,
            AddonVisibility::Available => Self::Available,
        }
    }
}

/// List the add-ons, with their prices per unit.
#[utoipa::path(get, path = "/addons", tag = "plans",
    responses((status = OK, body = Vec<AddonDto>), (status = UNAUTHORIZED, body = ErrorBody)))]
async fn list_addons(
    State(state): State<ApiState>,
    _current_user: CurrentUser,
) -> ApiResult<Json<Vec<AddonDto>>> {
    let catalog = sold(&state).await?;
    let mut dtos = Vec::new();
    for addon in db::billing::list_addons(&state.db).await? {
        let prices = prices(catalog.as_ref(), ItemKind::Addon, &addon.code);
        dtos.push(AddonDto {
            code: addon.code,
            name: addon.name,
            description: addon.description,
            unit: addon.unit,
            max_quantity: addon.max_quantity,
            // Not for sale without a price either.
            status: match addon.status.into() {
                AddonVisibility::Available if prices.is_empty() => AddonVisibility::ComingSoon,
                status => status,
            },
            prices,
        });
    }
    Ok(Json(dtos))
}

/// Get a workspace's plan, how much of it is used, and its subscription.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/plan", tag = "plans",
    params(("workspace_id" = Uuid, Path)),
    responses((status = OK, body = WorkspacePlanDto), (status = FORBIDDEN, body = ErrorBody)))]
async fn workspace_plan(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<AccountView>,
) -> ApiResult<Json<WorkspacePlanDto>> {
    let access = authorized.into_access();
    let mut dto = workspace_plan_dto(&state, access.workspace_id).await?;
    dto.can_manage_billing = access
        .can(&state, WorkspacePermission::BILLING_MANAGE)
        .await?;
    // A customer of a client workspace that has no share of its own would read the
    // organization's whole pool as its limit.
    if access.via == Via::Member {
        let workspace = db::workspaces::get_by_id(&state.db, access.workspace_id)
            .await?
            .ok_or_else(|| ApiError::not_found("Workspace not found"))?;
        if workspace.organization_id.is_some() && workspace.switcher_allocation.is_none() {
            dto.plan.max_switchers = None;
        }
    }
    // Back from the portal with a new card? Only those who can add one ask Stripe.
    if let (true, true, Some(billing)) = (
        dto.needs_payment_method,
        dto.can_manage_billing,
        &state.billing,
    ) {
        let workspace = db::workspaces::get_by_id(&state.db, access.workspace_id)
            .await?
            .ok_or_else(|| ApiError::not_found("Workspace not found"))?;
        match payment_method_missing(&state, billing, &workspace).await {
            Ok(missing) => dto.needs_payment_method = missing,
            Err(error) => tracing::warn!(?error, "Couldn't check for a card; keeping the reminder"),
        }
    }

    Ok(Json(dto))
}
