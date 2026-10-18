//! Paying with Stripe: Checkout to start a subscription (a plan and/or add-ons),
//! changes to it (add-on quantities, the plan, the billing period), the customer
//! portal for the card, invoices and cancelling, and the webhook that mirrors
//! subscriptions into the database.
//!
//! Raising something is charged now for the rest of the period; lowering it
//! applies at the end of the period (Stripe bills less from the next invoice and
//! a hold keeps the workspace entitled until then). See [`billing::quantity_change`].

use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::post,
};
use common::shutdown::ShutdownHandler;
use db::billing::ItemKind;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    billing::{self, Billing, BillingInterval, Catalog, HoldChange, Proration},
    permissions::{BillingManage, WorkspaceAuthorized},
    routes::{
        ApiError, ApiResult, ErrorBody,
        plans::{WorkspacePlanDto, workspace_plan_dto},
    },
    stripe,
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(checkout))
        .routes(routes!(portal))
        .routes(routes!(change_addon))
        .routes(routes!(change_plan))
        .routes(routes!(change_interval))
        .routes(routes!(resume_subscription))
        // Called by Stripe, not the dashboard; not part of the JSON API.
        .route("/billing/webhook", post(webhook))
}

/// How often ended holds are cleaned up (the renewal webhook normally does it).
const UPKEEP_EVERY: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddonQuantity {
    pub code: String,
    pub quantity: i32,
}

/// What to subscribe to: a plan, add-ons, or both, all billed per `interval`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CheckoutRequest {
    pub plan_code: Option<String>,
    pub interval: BillingInterval,
    #[serde(default)]
    pub addons: Vec<AddonQuantity>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct QuantityRequest {
    /// 0 removes it (at the end of the period).
    pub quantity: i32,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PlanRequest {
    /// `free` drops the plan at the end of the period, keeping any add-ons.
    pub plan_code: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct IntervalRequest {
    pub interval: BillingInterval,
}

/// Where to send the browser next.
#[derive(Debug, Serialize, ToSchema)]
pub struct RedirectDto {
    pub url: String,
}

fn enabled(state: &ApiState) -> ApiResult<&Arc<Billing>> {
    state.billing.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "billing_disabled",
            "Billing isn't set up on this server.",
        )
    })
}

fn stripe_error(error: stripe::Error) -> ApiError {
    // A declined card or a payment that needs the customer's bank: say so.
    if let stripe::Error::Api {
        status: 402,
        message,
        ..
    } = &error
    {
        return ApiError::new(
            StatusCode::PAYMENT_REQUIRED,
            "payment_failed",
            format!(
                "{message} Nothing was changed. You can update your card under Manage billing."
            ),
        );
    }
    tracing::warn!(%error, "Stripe request failed");
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        "stripe_error",
        "Couldn't reach the payment provider. Try again in a moment.",
    )
}

/// Whether the workspace is still without a card since an ownership transfer
/// removed the previous owner's. Asks Stripe only while that flag is set, and
/// clears it once the new owner has added one.
pub async fn payment_method_missing(
    state: &ApiState,
    billing: &Billing,
    workspace: &db::workspaces::Workspace,
) -> ApiResult<bool> {
    let (true, Some(customer)) = (
        workspace.needs_payment_method,
        workspace.stripe_customer_id.as_deref(),
    ) else {
        return Ok(false);
    };
    let has_one = billing
        .stripe
        .has_payment_method(customer)
        .await
        .map_err(stripe_error)?;
    if has_one {
        db::workspaces::set_needs_payment_method(&state.db, workspace.id, false).await?;
    }
    Ok(!has_one)
}

/// Charging now needs a card; after a transfer there may be none yet.
async fn ensure_payment_method(
    state: &ApiState,
    billing: &Billing,
    workspace_id: Uuid,
) -> ApiResult<()> {
    let workspace = db::workspaces::get_by_id(&state.db, workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Workspace not found"))?;
    if payment_method_missing(state, billing, &workspace).await? {
        return Err(ApiError::new(
            StatusCode::PAYMENT_REQUIRED,
            "no_payment_method",
            "There's no card on file for this workspace. Add one under Manage billing first.",
        ));
    }
    Ok(())
}

fn sync_error(error: String) -> ApiError {
    tracing::error!(%error, "Couldn't store a Stripe subscription");
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal",
        "Internal server error",
    )
}

fn plan_page(state: &ApiState, url_slug: &str) -> String {
    format!("{}/{url_slug}/settings/plan", state.dashboard_url)
}

/// The add-on, if it can be bought with `interval` billing.
async fn sellable_addon<'a>(
    state: &ApiState,
    catalog: &'a Catalog,
    code: &str,
    interval: BillingInterval,
) -> ApiResult<(db::billing::AddonProduct, &'a str)> {
    let addon = db::billing::get_addon(&state.db, code)
        .await?
        .ok_or_else(|| ApiError::not_found("No such add-on"))?;
    let why = match addon.status {
        db::billing::AddonVisibility::Available => None,
        db::billing::AddonVisibility::Unavailable => Some("is currently unavailable"),
        db::billing::AddonVisibility::ComingSoon => Some("isn't available yet"),
        db::billing::AddonVisibility::Hidden => Some("isn't for sale"),
    };
    if let Some(why) = why {
        return Err(ApiError::bad_request(
            "addon_unavailable",
            format!("{} {why}.", addon.name),
        ));
    }
    let price = catalog
        .price(ItemKind::Addon, code, interval)
        .ok_or_else(|| {
            ApiError::bad_request(
                "not_sold",
                format!("{} isn't sold with that billing period.", addon.name),
            )
        })?;
    Ok((addon, price))
}

fn check_quantity(addon: &db::billing::AddonProduct, quantity: i32, min: i32) -> ApiResult<()> {
    if quantity < min || quantity > addon.max_quantity {
        return Err(ApiError::bad_request(
            "invalid_quantity",
            format!(
                "Choose between {min} and {} for {}.",
                addon.max_quantity, addon.name
            ),
        ));
    }
    Ok(())
}

/// Start paying: returns a Stripe Checkout page to send the browser to.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/billing/checkout", tag = "billing",
    params(("workspace_id" = Uuid, Path)),
    request_body = CheckoutRequest,
    responses(
        (status = OK, body = RedirectDto),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = CONFLICT, body = ErrorBody, description = "Already subscribed; change the subscription instead"),
        (status = SERVICE_UNAVAILABLE, body = ErrorBody),
    ))]
async fn checkout(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<BillingManage>,
    Json(request): Json<CheckoutRequest>,
) -> ApiResult<Json<RedirectDto>> {
    let billing = enabled(&state)?;
    let catalog = Catalog::load(&state.db).await?;
    let access = authorized.into_access();
    state.limits.outbound(access.user.id())?;
    let workspace_id = access.workspace_id;

    let mut items: Vec<(String, i32)> = Vec::new();
    if let Some(plan_code) = &request.plan_code {
        let price = catalog
            .plan_price(plan_code, request.interval)
            .ok_or_else(|| {
                ApiError::bad_request(
                    "invalid_plan",
                    "That plan isn't sold with that billing period.",
                )
            })?;
        items.push((price.to_owned(), 1));
    }
    for wanted in &request.addons {
        let (addon, price) =
            sellable_addon(&state, &catalog, &wanted.code, request.interval).await?;
        check_quantity(&addon, wanted.quantity, 1)?;
        items.push((price.to_owned(), wanted.quantity));
    }
    if items.is_empty() {
        return Err(ApiError::bad_request(
            "nothing_to_buy",
            "Choose a plan or an add-on.",
        ));
    }

    if db::billing::live_subscription(&state.db, workspace_id)
        .await?
        .is_some()
    {
        return Err(ApiError::conflict(
            "already_subscribed",
            "This workspace already has a subscription. Change it here instead.",
        ));
    }

    let workspace = db::workspaces::get_by_id(&state.db, workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Workspace not found"))?;
    let customer = match workspace.stripe_customer_id {
        Some(customer) => customer,
        None => {
            let created = billing
                .stripe
                .create_customer(&workspace.name, workspace.id)
                .await
                .map_err(stripe_error)?;
            db::billing::set_customer_if_missing(&state.db, workspace.id, &created.id)
                .await?
                .ok_or_else(|| ApiError::not_found("Workspace not found"))?
        }
    };

    let page = plan_page(&state, &workspace.url_slug);
    let session = billing
        .stripe
        .create_checkout_session(&stripe::CheckoutParams {
            customer: &customer,
            items: &items,
            workspace_id: workspace.id,
            success_url: &format!("{page}?checkout=success"),
            cancel_url: &format!("{page}?checkout=canceled"),
        })
        .await
        .map_err(stripe_error)?;

    Ok(Json(RedirectDto { url: session.url }))
}

/// The card, invoices and cancelling everything, in Stripe's portal.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/billing/portal", tag = "billing",
    params(("workspace_id" = Uuid, Path)),
    responses(
        (status = OK, body = RedirectDto),
        (status = CONFLICT, body = ErrorBody, description = "Never subscribed"),
        (status = SERVICE_UNAVAILABLE, body = ErrorBody),
    ))]
async fn portal(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<BillingManage>,
) -> ApiResult<Json<RedirectDto>> {
    let billing = enabled(&state)?;
    let access = authorized.into_access();
    state.limits.outbound(access.user.id())?;
    let workspace = db::workspaces::get_by_id(&state.db, access.workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Workspace not found"))?;
    let customer = workspace.stripe_customer_id.ok_or_else(|| {
        ApiError::conflict(
            "no_customer",
            "This workspace hasn't subscribed to anything yet.",
        )
    })?;

    let session = billing
        .stripe
        .create_portal_session(&customer, &plan_page(&state, &workspace.url_slug))
        .await
        .map_err(stripe_error)?;

    Ok(Json(RedirectDto { url: session.url }))
}

/// The live subscription, ready to change.
struct Live {
    subscription: db::billing::Subscription,
    items: Vec<db::billing::Item>,
}

async fn live_for_change(state: &ApiState, workspace_id: Uuid) -> ApiResult<Option<Live>> {
    let Some(subscription) = db::billing::live_subscription(&state.db, workspace_id).await? else {
        return Ok(None);
    };
    if subscription.cancel_at_period_end {
        return Err(ApiError::conflict(
            "subscription_ending",
            "The subscription is set to end. Resume it first.",
        ));
    }
    let items = db::billing::items(&state.db, subscription.id).await?;
    Ok(Some(Live {
        subscription,
        items,
    }))
}

fn no_subscription() -> ApiError {
    ApiError::conflict(
        "no_subscription",
        "This workspace has no subscription yet. Start one with Checkout.",
    )
}

fn proration_form(proration: Proration) -> Vec<(String, String)> {
    match proration {
        Proration::None => vec![("proration_behavior".into(), "none".into())],
        Proration::ChargeNow => vec![
            ("proration_behavior".into(), "always_invoice".into()),
            // A declined card fails the request instead of leaving it unpaid.
            ("payment_behavior".into(), "error_if_incomplete".into()),
        ],
    }
}

/// Moves one plan or add-on on the live subscription to `target`, charging or
/// holding as [`billing::quantity_change`] says, and stores the result.
async fn change_quantity(
    state: &ApiState,
    billing: &Billing,
    catalog: &Catalog,
    live: &Live,
    kind: ItemKind,
    code: &str,
    price: &str,
    target: i32,
) -> ApiResult<()> {
    let is_this = |item: &db::billing::Item| item.kind == kind && item.code == code;
    let item = live.items.iter().find(|item| is_this(item));
    let billed = item.map_or(0, |item| item.quantity);
    let held = db::billing::hold(&state.db, live.subscription.id, kind, code)
        .await?
        .map_or(0, |hold| hold.quantity);
    let change = billing::quantity_change(billed, held, target);
    let stripe_id = &live.subscription.stripe_subscription_id;
    if change
        .steps
        .iter()
        .any(|(_, proration)| *proration == Proration::ChargeNow)
    {
        ensure_payment_method(state, billing, live.subscription.workspace_id).await?;
    }

    // Stripe needs at least one item: removing the last one ends the whole
    // subscription at the period end, which keeps everything until then.
    if target == 0 && item.is_some() && live.items.iter().all(is_this) {
        let form = vec![("cancel_at_period_end".to_owned(), "true".to_owned())];
        let subscription = billing
            .stripe
            .update_subscription(stripe_id, &form)
            .await
            .map_err(stripe_error)?;
        return apply_stripe(state, &subscription).await.map_err(sync_error);
    }

    // Keep what's paid for before Stripe stops billing it, so a webhook racing
    // this request never sees the workspace without it.
    if let HoldChange::Set(quantity) = change.hold {
        let until = live
            .subscription
            .current_period_end
            .unwrap_or_else(chrono::Utc::now);
        db::billing::set_hold(&state.db, live.subscription.id, kind, code, quantity, until).await?;
    }

    let mut item_id = item.map(|item| item.stripe_item_id.clone());
    let mut latest = None;
    for (quantity, proration) in change.steps {
        let mut form: Vec<(String, String)> = match (&item_id, quantity) {
            (None, 0) => continue,
            (Some(id), 0) => vec![
                ("items[0][id]".into(), id.clone()),
                ("items[0][deleted]".into(), "true".into()),
            ],
            (Some(id), quantity) => vec![
                ("items[0][id]".into(), id.clone()),
                ("items[0][quantity]".into(), quantity.to_string()),
            ],
            (None, quantity) => vec![
                ("items[0][price]".into(), price.to_owned()),
                ("items[0][quantity]".into(), quantity.to_string()),
            ],
        };
        form.extend(proration_form(proration));
        let subscription = billing
            .stripe
            .update_subscription(stripe_id, &form)
            .await
            .map_err(stripe_error)?;
        // A new item gets its ID now; a later step changes that one.
        item_id = subscription
            .items
            .data
            .iter()
            .find(|item| {
                catalog
                    .lookup(&item.price.id)
                    .is_some_and(|entry| entry.kind == kind && entry.code == code)
            })
            .map(|item| item.id.clone());
        latest = Some(subscription);
    }

    if change.hold == HoldChange::Clear {
        db::billing::clear_hold(&state.db, live.subscription.id, kind, code).await?;
    }
    if let Some(subscription) = latest {
        apply_stripe(state, &subscription)
            .await
            .map_err(sync_error)?;
    }
    Ok(())
}

async fn fresh_plan(state: &ApiState, workspace_id: Uuid) -> ApiResult<Json<WorkspacePlanDto>> {
    let mut dto = workspace_plan_dto(state, workspace_id).await?;
    dto.can_manage_billing = true;
    Ok(Json(dto))
}

/// Set how many of an add-on the workspace has. More is charged now for the
/// rest of the period; fewer (or 0) applies when the period ends.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/billing/addons/{addon_code}", tag = "billing",
    params(("workspace_id" = Uuid, Path), ("addon_code" = String, Path)),
    request_body = QuantityRequest,
    responses(
        (status = OK, body = WorkspacePlanDto),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = PAYMENT_REQUIRED, body = ErrorBody, description = "`payment_failed` (the charge was declined) or `no_payment_method`; nothing changed"),
        (status = CONFLICT, body = ErrorBody, description = "No subscription yet, or it is ending"),
    ))]
async fn change_addon(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<BillingManage>,
    Path(path): Path<(Uuid, String)>,
    Json(request): Json<QuantityRequest>,
) -> ApiResult<Json<WorkspacePlanDto>> {
    let (_, addon_code) = path;
    let billing = enabled(&state)?;
    let catalog = Catalog::load(&state.db).await?;
    let workspace_id = authorized.into_access().workspace_id;
    let live = live_for_change(&state, workspace_id)
        .await?
        .ok_or_else(no_subscription)?;
    let interval = BillingInterval::parse(&live.subscription.billing_interval)
        .unwrap_or(BillingInterval::Month);

    let addon = db::billing::get_addon(&state.db, &addon_code)
        .await?
        .ok_or_else(|| ApiError::not_found("No such add-on"))?;
    check_quantity(&addon, request.quantity, 0)?;
    // Lowering what you have is always allowed; buying more needs it on sale.
    let price = if request.quantity > 0 {
        sellable_addon(&state, &catalog, &addon_code, interval)
            .await?
            .1
    } else {
        catalog
            .price(ItemKind::Addon, &addon_code, interval)
            .unwrap_or_default()
    };

    change_quantity(
        &state,
        billing,
        &catalog,
        &live,
        ItemKind::Addon,
        &addon_code,
        price,
        request.quantity,
    )
    .await?;
    fresh_plan(&state, workspace_id).await
}

/// Move to another plan on an existing subscription. A paid plan is charged
/// now for the rest of the period; `free` applies when the period ends (add-ons
/// stay).
#[utoipa::path(put, path = "/workspaces/{workspace_id}/billing/plan", tag = "billing",
    params(("workspace_id" = Uuid, Path)),
    request_body = PlanRequest,
    responses(
        (status = OK, body = WorkspacePlanDto),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = PAYMENT_REQUIRED, body = ErrorBody, description = "`payment_failed` (the charge was declined) or `no_payment_method`; nothing changed"),
        (status = CONFLICT, body = ErrorBody, description = "No subscription yet (use Checkout), or it is ending"),
    ))]
async fn change_plan(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<BillingManage>,
    Json(request): Json<PlanRequest>,
) -> ApiResult<Json<WorkspacePlanDto>> {
    let billing = enabled(&state)?;
    let catalog = Catalog::load(&state.db).await?;
    let workspace_id = authorized.into_access().workspace_id;
    let paid = request.plan_code != db::plans::FREE;
    let Some(live) = live_for_change(&state, workspace_id).await? else {
        return if paid {
            Err(no_subscription())
        } else {
            fresh_plan(&state, workspace_id).await
        };
    };
    let interval = BillingInterval::parse(&live.subscription.billing_interval)
        .unwrap_or(BillingInterval::Month);
    let current = live.items.iter().find(|item| item.kind == ItemKind::Plan);

    if !paid {
        if let Some(current) = current {
            change_quantity(
                &state,
                billing,
                &catalog,
                &live,
                ItemKind::Plan,
                &current.code,
                &current.stripe_price_id,
                0,
            )
            .await?;
        }
        return fresh_plan(&state, workspace_id).await;
    }

    let price = catalog
        .plan_price(&request.plan_code, interval)
        .ok_or_else(|| {
            ApiError::bad_request(
                "invalid_plan",
                "That plan isn't sold with this billing period.",
            )
        })?;
    match current {
        Some(current) if current.code == request.plan_code => {}
        // Another paid plan: swap its price, charged now.
        Some(current) => {
            ensure_payment_method(&state, billing, workspace_id).await?;
            let mut form = vec![
                ("items[0][id]".to_owned(), current.stripe_item_id.clone()),
                ("items[0][price]".to_owned(), price.to_owned()),
            ];
            form.extend(proration_form(Proration::ChargeNow));
            let subscription = billing
                .stripe
                .update_subscription(&live.subscription.stripe_subscription_id, &form)
                .await
                .map_err(stripe_error)?;
            apply_stripe(&state, &subscription)
                .await
                .map_err(sync_error)?;
        }
        None => {
            change_quantity(
                &state,
                billing,
                &catalog,
                &live,
                ItemKind::Plan,
                &request.plan_code,
                price,
                1,
            )
            .await?;
        }
    }
    fresh_plan(&state, workspace_id).await
}

/// Switch between monthly and yearly billing for everything, now: unused time
/// is credited and the new period charged.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/billing/interval", tag = "billing",
    params(("workspace_id" = Uuid, Path)),
    request_body = IntervalRequest,
    responses(
        (status = OK, body = WorkspacePlanDto),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = PAYMENT_REQUIRED, body = ErrorBody, description = "`payment_failed` (the charge was declined) or `no_payment_method`; nothing changed"),
        (status = CONFLICT, body = ErrorBody, description = "No subscription, or it is ending"),
    ))]
async fn change_interval(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<BillingManage>,
    Json(request): Json<IntervalRequest>,
) -> ApiResult<Json<WorkspacePlanDto>> {
    let billing = enabled(&state)?;
    let catalog = Catalog::load(&state.db).await?;
    let workspace_id = authorized.into_access().workspace_id;
    let live = live_for_change(&state, workspace_id)
        .await?
        .ok_or_else(no_subscription)?;
    if live.subscription.billing_interval == request.interval.as_str() {
        return fresh_plan(&state, workspace_id).await;
    }
    ensure_payment_method(&state, billing, workspace_id).await?;

    let mut form = Vec::new();
    for (index, item) in live.items.iter().enumerate() {
        let price = catalog
            .price(item.kind, &item.code, request.interval)
            .ok_or_else(|| {
                ApiError::bad_request(
                    "not_sold",
                    "Something in this subscription isn't sold with that billing period.",
                )
            })?;
        form.push((format!("items[{index}][id]"), item.stripe_item_id.clone()));
        form.push((format!("items[{index}][price]"), price.to_owned()));
    }
    form.extend(proration_form(Proration::ChargeNow));
    let subscription = billing
        .stripe
        .update_subscription(&live.subscription.stripe_subscription_id, &form)
        .await
        .map_err(stripe_error)?;
    apply_stripe(&state, &subscription)
        .await
        .map_err(sync_error)?;
    fresh_plan(&state, workspace_id).await
}

/// Undo a cancellation before the period ends.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/billing/resume", tag = "billing",
    params(("workspace_id" = Uuid, Path)),
    responses(
        (status = OK, body = WorkspacePlanDto),
        (status = CONFLICT, body = ErrorBody, description = "No subscription"),
    ))]
async fn resume_subscription(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<BillingManage>,
) -> ApiResult<Json<WorkspacePlanDto>> {
    let billing = enabled(&state)?;
    let workspace_id = authorized.into_access().workspace_id;
    let subscription = db::billing::live_subscription(&state.db, workspace_id)
        .await?
        .ok_or_else(no_subscription)?;
    if subscription.cancel_at_period_end {
        let form = vec![("cancel_at_period_end".to_owned(), "false".to_owned())];
        let updated = billing
            .stripe
            .update_subscription(&subscription.stripe_subscription_id, &form)
            .await
            .map_err(stripe_error)?;
        apply_stripe(&state, &updated).await.map_err(sync_error)?;
    }
    fresh_plan(&state, workspace_id).await
}

/// Stripe's webhook. Every event about a subscription triggers a fresh read of
/// it from Stripe, so handling is idempotent and event order doesn't matter.
/// Transient failures answer 500 so Stripe retries.
async fn webhook(State(state): State<ApiState>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let Some(billing) = state.billing.clone() else {
        return StatusCode::NOT_FOUND;
    };
    let signature = headers
        .get("stripe-signature")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let now = chrono::Utc::now().timestamp();
    if let Err(error) = stripe::verify_webhook(signature, &body, &billing.webhook_secret, now) {
        tracing::warn!(%error, "Rejected a Stripe webhook");
        return StatusCode::BAD_REQUEST;
    }
    let event: stripe::Event = match serde_json::from_slice(&body) {
        Ok(event) => event,
        Err(error) => {
            tracing::warn!(%error, "Couldn't read a Stripe webhook event");
            return StatusCode::BAD_REQUEST;
        }
    };

    let Some(subscription_id) = event.subscription_id() else {
        return StatusCode::OK;
    };
    let result = match billing.stripe.get_subscription(subscription_id).await {
        Ok(subscription) => apply_stripe(&state, &subscription).await,
        Err(error) => Err(error.to_string()),
    };
    match result {
        Ok(()) => StatusCode::OK,
        Err(error) => {
            tracing::warn!(%error, event = %event.id, kind = %event.kind, "Stripe webhook failed; Stripe will retry");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

/// Mirrors a Stripe subscription and updates the workspace's plan. Returns an
/// error only for failures worth retrying.
async fn apply_stripe(state: &ApiState, subscription: &stripe::Subscription) -> Result<(), String> {
    let from_metadata = subscription
        .metadata
        .get("workspace_id")
        .and_then(|id| id.parse::<Uuid>().ok());
    let workspace_id = match from_metadata {
        Some(id) => Some(id),
        None => db::billing::workspace_by_customer(&state.db, &subscription.customer)
            .await
            .map_err(|error| error.to_string())?,
    };
    let Some(workspace) = (match workspace_id {
        Some(id) => db::workspaces::get_by_id(&state.db, id)
            .await
            .map_err(|error| error.to_string())?,
        None => None,
    }) else {
        tracing::warn!(subscription = %subscription.id, "Stripe subscription for no known workspace");
        return Ok(());
    };
    // Only trust the metadata if the customer is the workspace's own.
    if workspace
        .stripe_customer_id
        .as_deref()
        .is_some_and(|customer| customer != subscription.customer)
    {
        tracing::warn!(
            subscription = %subscription.id,
            workspace_id = %workspace.id,
            "Stripe subscription names a workspace with another customer; ignoring it"
        );
        return Ok(());
    }

    let catalog = Catalog::load(&state.db)
        .await
        .map_err(|error| error.to_string())?;
    let sync = billing::to_sync(subscription, &catalog, workspace.id);
    match db::billing::apply_subscription(&state.db, &sync).await {
        Ok(Some(change)) => {
            tracing::info!(
                subscription = %subscription.id,
                workspace_id = %workspace.id,
                status = %sync.status,
                before = %change.before,
                after = %change.after,
                "Synced Stripe subscription"
            );
            if change.changed() {
                state.notify_workspace(workspace.id).await;
            }
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(error) if error.is_unique_violation() => {
            // Two live subscriptions (e.g. two checkouts at once). Retrying won't help.
            tracing::error!(
                subscription = %subscription.id,
                workspace_id = %workspace.id,
                "Workspace already has a live subscription; refund and cancel one in Stripe"
            );
            Ok(())
        }
        Err(error) => Err(error.to_string()),
    }
}

/// Ends holds whose period is over (normally the renewal webhook does it first)
/// and tells obs-link about workspaces whose plan changed.
pub fn spawn_upkeep(state: ApiState, mut shutdown: ShutdownHandler) {
    tokio::spawn(async move {
        let mut every = tokio::time::interval(UPKEEP_EVERY);
        loop {
            tokio::select! {
                _ = shutdown.changed() => return,
                _ = every.tick() => match db::billing::expire_holds(&state.db).await {
                    Ok(changes) => {
                        for (workspace_id, change) in changes {
                            tracing::info!(%workspace_id, before = %change.before, after = %change.after, "Plan changed at the end of the period");
                            state.notify_workspace(workspace_id).await;
                        }
                    }
                    Err(error) => tracing::warn!(%error, "Couldn't expire billing holds"),
                },
            }
        }
    });
}
