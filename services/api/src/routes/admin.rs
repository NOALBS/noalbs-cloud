//! The staff admin section: an overview, users and workspaces, and the plans
//! and add-ons for sale. Products and prices are created in Stripe from here and
//! kept in the database; changing a price archives the old one, and
//! subscriptions on it keep it (grandfathered).

use axum::{
    Json,
    extract::{Path, Query, State},
};
use db::billing::ItemKind;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    billing::BillingInterval,
    permissions::Staff,
    routes::{
        ApiError, ApiResult, ErrorBody, admin_actions,
        plans::{AddonVisibility, WorkspaceAddonDto},
        support::{self, SupportGrantDto},
        workspaces::WorkspaceRole,
    },
    stripe::{self, DashboardPage},
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(admin_overview))
        .routes(routes!(admin_list_users))
        .routes(routes!(admin_get_user))
        .routes(routes!(admin_set_staff))
        .routes(routes!(admin_list_workspaces))
        .routes(routes!(admin_get_workspace))
        .routes(routes!(admin_set_workspace_plan))
        .routes(routes!(admin_products))
        .routes(routes!(admin_create_plan))
        .routes(routes!(admin_update_plan, admin_delete_plan))
        .routes(routes!(admin_create_addon))
        .routes(routes!(admin_update_addon, admin_delete_addon))
        .routes(routes!(admin_set_price, admin_archive_price))
        .routes(routes!(admin_link_price))
}

const PER_PAGE: i64 = 25;
/// Most add-on units one workspace may buy.
const MAX_ADDON_QUANTITY: i32 = 100;

fn stripe_link(state: &ApiState, page: DashboardPage, id: Option<&str>) -> Option<String> {
    let billing = state.billing.as_ref()?;
    Some(billing.stripe.dashboard_url(page, id?))
}

fn stripe_error(error: stripe::Error) -> ApiError {
    tracing::warn!(%error, "Stripe request failed");
    match error {
        stripe::Error::Api {
            status: 400..=499,
            message,
            ..
        } => ApiError::bad_request("stripe_rejected", format!("Stripe: {message}")),
        _ => ApiError::new(
            axum::http::StatusCode::BAD_GATEWAY,
            "stripe_error",
            "Couldn't reach Stripe. Try again in a moment.",
        ),
    }
}

// --- Overview --------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema)]
pub struct CountDto {
    pub code: String,
    pub name: String,
    pub count: i64,
}

/// An amount in a currency's smallest unit.
#[derive(Debug, Serialize, ToSchema)]
pub struct MoneyDto {
    pub amount: i64,
    pub currency: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminOverviewDto {
    pub users: i64,
    pub staff: i64,
    pub new_users_7d: i64,
    pub workspaces: i64,
    /// Workspaces with a live subscription.
    pub paying_workspaces: i64,
    /// Workspaces per plan.
    pub plans: Vec<CountDto>,
    /// Units of each add-on billed on live subscriptions.
    pub addons: Vec<CountDto>,
    /// Monthly recurring revenue (yearly prices ÷ 12), per currency.
    pub mrr: Vec<MoneyDto>,
    pub billing_enabled: bool,
    /// Whether the Stripe key is a test-mode key.
    pub stripe_test_mode: bool,
    pub newest_users: Vec<AdminUserDto>,
}

/// Numbers at a glance.
#[utoipa::path(get, path = "/admin/overview", tag = "admin",
    responses((status = OK, body = AdminOverviewDto), (status = FORBIDDEN, body = ErrorBody)))]
async fn admin_overview(
    State(state): State<ApiState>,
    _staff: Staff,
) -> ApiResult<Json<AdminOverviewDto>> {
    let overview = db::admin::overview(&state.db).await?;
    let (newest, _) = db::admin::list_users(&state.db, Default::default(), 5, 0).await?;
    let stripe_test_mode = state.billing.as_ref().is_some_and(|billing| {
        billing
            .stripe
            .dashboard_url(DashboardPage::Customer, "x")
            .contains("/test/")
    });

    Ok(Json(AdminOverviewDto {
        users: overview.counts.users,
        staff: overview.counts.staff,
        new_users_7d: overview.counts.new_users_7d,
        workspaces: overview.counts.workspaces,
        paying_workspaces: overview.counts.paying_workspaces,
        plans: overview
            .plans
            .into_iter()
            .map(|plan| CountDto {
                code: plan.code,
                name: plan.name,
                count: plan.workspaces,
            })
            .collect(),
        addons: overview
            .addons
            .into_iter()
            .map(|addon| CountDto {
                code: addon.code,
                name: addon.name,
                count: addon.units,
            })
            .collect(),
        mrr: overview
            .mrr
            .into_iter()
            .map(|mrr| MoneyDto {
                amount: mrr.amount,
                currency: mrr.currency,
            })
            .collect(),
        billing_enabled: state.billing.is_some(),
        stripe_test_mode,
        newest_users: newest.into_iter().map(Into::into).collect(),
    }))
}

// --- Users -----------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminUserDto {
    pub id: Uuid,
    /// From the most recently used sign-in.
    pub username: Option<String>,
    pub provider: Option<String>,
    pub avatar_url: Option<String>,
    pub staff: bool,
    /// Disabled by staff (no sign-in): when and why; `null` otherwise.
    pub disabled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub disabled_reason: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub workspaces: i64,
    pub last_active_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<db::admin::UserRow> for AdminUserDto {
    fn from(user: db::admin::UserRow) -> Self {
        Self {
            id: user.id,
            username: user.username,
            provider: user.provider,
            avatar_url: user.avatar_url,
            staff: user.staff,
            disabled_at: user.disabled_at,
            disabled_reason: user.disabled_reason,
            created_at: user.created_at,
            workspaces: user.workspaces,
            last_active_at: user.last_active_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminUserPage {
    pub items: Vec<AdminUserDto>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct ListQuery {
    /// Search text (name, slug or ID).
    pub q: Option<String>,
    /// 1-based.
    pub page: Option<i64>,
    /// Only the client workspaces of this organization (workspaces list).
    pub organization: Option<Uuid>,
}

impl ListQuery {
    fn search(&self) -> Option<&str> {
        self.q.as_deref().map(str::trim).filter(|q| !q.is_empty())
    }

    fn page(&self) -> i64 {
        self.page.unwrap_or(1).max(1)
    }
}

/// How recently the listed users were active (used a session or signed in).
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UserActivity {
    /// Within the last 24 hours.
    Day,
    Week,
    Month,
    /// Not within the last 30 days, or never.
    Inactive,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UserSort {
    /// Newest first.
    #[default]
    Joined,
    /// Most recently active first.
    Active,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct UserListQuery {
    /// Search text (a username or ID).
    pub q: Option<String>,
    /// 1-based.
    pub page: Option<i64>,
    #[param(inline)]
    pub active: Option<UserActivity>,
    #[param(inline)]
    pub sort: Option<UserSort>,
}

/// Users, newest first unless sorted by activity.
#[utoipa::path(get, path = "/admin/users", tag = "admin", params(UserListQuery),
    responses((status = OK, body = AdminUserPage), (status = FORBIDDEN, body = ErrorBody)))]
async fn admin_list_users(
    State(state): State<ApiState>,
    _staff: Staff,
    Query(query): Query<UserListQuery>,
) -> ApiResult<Json<AdminUserPage>> {
    let list = ListQuery {
        q: query.q,
        page: query.page,
        organization: None,
    };
    let page = list.page();
    let now = chrono::Utc::now();
    let days_ago = |days| Some(now - chrono::TimeDelta::days(days));
    let mut filter = db::admin::UserFilter {
        query: list.search(),
        by_activity: matches!(query.sort, Some(UserSort::Active)),
        ..Default::default()
    };
    match query.active {
        Some(UserActivity::Day) => filter.active_since = days_ago(1),
        Some(UserActivity::Week) => filter.active_since = days_ago(7),
        Some(UserActivity::Month) => filter.active_since = days_ago(30),
        Some(UserActivity::Inactive) => filter.inactive_since = days_ago(30),
        None => {}
    }
    let (users, total) =
        db::admin::list_users(&state.db, filter, PER_PAGE, (page - 1) * PER_PAGE).await?;
    Ok(Json(AdminUserPage {
        items: users.into_iter().map(Into::into).collect(),
        total,
        page,
        per_page: PER_PAGE,
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminConnectionDto {
    pub provider: String,
    pub username: String,
    pub avatar_url: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminUserWorkspaceDto {
    pub id: Uuid,
    pub name: String,
    pub url_slug: String,
    pub plan_code: String,
    pub role: Option<WorkspaceRole>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminUserDetailDto {
    pub user: AdminUserDto,
    pub connections: Vec<AdminConnectionDto>,
    pub workspaces: Vec<AdminUserWorkspaceDto>,
}

pub(crate) async fn user_detail(state: &ApiState, user_id: Uuid) -> ApiResult<AdminUserDetailDto> {
    let user = db::admin::get_user(&state.db, user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("User not found"))?;
    let connections = db::oauth2_connections::list_profiles_for_user(&state.db, user_id).await?;
    let workspaces = db::workspaces::list_summaries(&state.db, user_id).await?;

    Ok(AdminUserDetailDto {
        user: user.into(),
        connections: connections
            .into_iter()
            .map(|connection| AdminConnectionDto {
                provider: connection.provider,
                username: connection.provider_username,
                avatar_url: connection.provider_avatar_url,
                created_at: connection.created_at,
            })
            .collect(),
        workspaces: workspaces
            .into_iter()
            .map(|workspace| AdminUserWorkspaceDto {
                id: workspace.id,
                name: workspace.name,
                url_slug: workspace.url_slug,
                plan_code: workspace.plan_code,
                role: workspace.role.map(Into::into),
            })
            .collect(),
    })
}

/// One user: sign-ins and workspaces.
#[utoipa::path(get, path = "/admin/users/{user_id}", tag = "admin",
    params(("user_id" = Uuid, Path)),
    responses((status = OK, body = AdminUserDetailDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_get_user(
    State(state): State<ApiState>,
    _staff: Staff,
    Path(user_id): Path<Uuid>,
) -> ApiResult<Json<AdminUserDetailDto>> {
    Ok(Json(user_detail(&state, user_id).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetStaff {
    pub staff: bool,
}

/// Grant or remove staff access. You can't remove your own.
#[utoipa::path(put, path = "/admin/users/{user_id}/staff", tag = "admin",
    params(("user_id" = Uuid, Path)),
    request_body = SetStaff,
    responses((status = OK, body = AdminUserDetailDto), (status = NOT_FOUND, body = ErrorBody),
        (status = CONFLICT, body = ErrorBody, description = "Removing your own staff access")))]
async fn admin_set_staff(
    State(state): State<ApiState>,
    Staff(me): Staff,
    Path(user_id): Path<Uuid>,
    Json(request): Json<SetStaff>,
) -> ApiResult<Json<AdminUserDetailDto>> {
    if user_id == me.id() && !request.staff {
        return Err(ApiError::conflict(
            "own_staff",
            "You can't remove your own staff access. Ask another staff member.",
        ));
    }
    if !db::admin::set_staff(&state.db, user_id, request.staff).await? {
        return Err(ApiError::not_found("User not found"));
    }
    admin_actions::record(
        &state,
        &me,
        admin_actions::Action {
            action: "set_staff",
            target_user: Some(user_id),
            details: serde_json::json!({ "staff": request.staff }),
            ..admin_actions::Action::default()
        },
    )
    .await?;
    Ok(Json(user_detail(&state, user_id).await?))
}

// --- Workspaces ------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminWorkspaceDto {
    pub id: Uuid,
    pub name: String,
    pub url_slug: String,
    pub plan_code: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub members: i64,
    pub switchers: i64,
    /// The live subscription's status, if it has one.
    pub subscription_status: Option<String>,
    pub owner_username: Option<String>,
    /// The Stripe customer in the Stripe Dashboard.
    pub stripe_customer_url: Option<String>,
    /// Suspended by staff: when and why; `null` otherwise.
    pub suspended_at: Option<chrono::DateTime<chrono::Utc>>,
    pub suspended_reason: Option<String>,
    /// The organization this is a client workspace of.
    pub organization: Option<AdminOrganizationRefDto>,
}

/// An organization, named where something belongs to it.
#[derive(Debug, Serialize, ToSchema)]
pub struct AdminOrganizationRefDto {
    pub id: Uuid,
    pub name: String,
}

fn workspace_dto(state: &ApiState, workspace: db::admin::WorkspaceRow) -> AdminWorkspaceDto {
    AdminWorkspaceDto {
        stripe_customer_url: stripe_link(
            state,
            DashboardPage::Customer,
            workspace.stripe_customer_id.as_deref(),
        ),
        id: workspace.id,
        name: workspace.name,
        url_slug: workspace.url_slug,
        plan_code: workspace.plan_code,
        created_at: workspace.created_at,
        members: workspace.members,
        switchers: workspace.switchers,
        subscription_status: workspace.subscription_status,
        owner_username: workspace.owner_username,
        suspended_at: workspace.suspended_at,
        suspended_reason: workspace.suspended_reason,
        organization: workspace
            .organization_id
            .zip(workspace.organization_name)
            .map(|(id, name)| AdminOrganizationRefDto { id, name }),
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminWorkspacePage {
    pub items: Vec<AdminWorkspaceDto>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

/// Workspaces, newest first.
#[utoipa::path(get, path = "/admin/workspaces", tag = "admin", params(ListQuery),
    responses((status = OK, body = AdminWorkspacePage), (status = FORBIDDEN, body = ErrorBody)))]
async fn admin_list_workspaces(
    State(state): State<ApiState>,
    _staff: Staff,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<AdminWorkspacePage>> {
    let page = query.page();
    let (workspaces, total) = db::admin::list_workspaces(
        &state.db,
        query.search(),
        query.organization,
        PER_PAGE,
        (page - 1) * PER_PAGE,
    )
    .await?;
    Ok(Json(AdminWorkspacePage {
        items: workspaces
            .into_iter()
            .map(|workspace| workspace_dto(&state, workspace))
            .collect(),
        total,
        page,
        per_page: PER_PAGE,
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminMemberDto {
    pub user_id: Uuid,
    pub username: Option<String>,
    pub provider: Option<String>,
    pub role: WorkspaceRole,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminSwitcherDto {
    pub id: Uuid,
    pub name: String,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
    pub stream_server_count: i64,
    pub paused: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminItemDto {
    /// `plan` or `addon`.
    pub kind: String,
    pub code: String,
    pub quantity: i32,
    /// What its price costs per unit, if the price is known.
    pub price: Option<MoneyDto>,
    /// On a price that is no longer sold (grandfathered).
    pub grandfathered: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminSubscriptionDto {
    pub status: String,
    pub interval: BillingInterval,
    pub current_period_end: Option<chrono::DateTime<chrono::Utc>>,
    pub cancel_at_period_end: bool,
    pub held_plan_code: Option<String>,
    pub items: Vec<AdminItemDto>,
    pub stripe_url: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminWorkspaceDetailDto {
    pub workspace: AdminWorkspaceDto,
    pub members: Vec<AdminMemberDto>,
    pub switchers: Vec<AdminSwitcherDto>,
    pub subscription: Option<AdminSubscriptionDto>,
    pub addons: Vec<WorkspaceAddonDto>,
    /// What NOALBS support may do in it now: its own permission, else
    /// break-glass; `null` = nothing (opening it asks for permission).
    pub support_access: Option<SupportGrantDto>,
    /// Chat channels linked to its switchers, with every link they have
    /// (other workspaces' too).
    pub chat_channels: Vec<AdminChatChannelDto>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminChatChannelDto {
    pub id: Uuid,
    /// `twitch` or `kick`.
    pub platform: String,
    pub login: String,
    pub display_name: String,
    /// `active` or `revoked`.
    pub status: String,
    pub links: Vec<AdminChatLinkDto>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminChatLinkDto {
    pub switcher_id: Uuid,
    pub switcher_name: String,
    pub workspace_id: Uuid,
    pub workspace_name: String,
}

pub(crate) async fn workspace_detail(
    state: &ApiState,
    workspace_id: Uuid,
) -> ApiResult<AdminWorkspaceDetailDto> {
    let workspace = db::admin::get_workspace(&state.db, workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Workspace not found"))?;
    let members = db::workspaces::list_member_profiles(&state.db, workspace_id).await?;
    let switchers = db::switchers::list_summaries(&state.db, workspace_id).await?;
    let addons = db::billing::addon_statuses(&state.db, workspace_id).await?;

    let subscription = match db::billing::live_subscription(&state.db, workspace_id).await? {
        Some(subscription) => {
            let prices = db::prices::all(&state.db).await?;
            let items = db::billing::items(&state.db, subscription.id)
                .await?
                .into_iter()
                .map(|item| {
                    let price = prices
                        .iter()
                        .find(|price| price.stripe_price_id == item.stripe_price_id);
                    AdminItemDto {
                        kind: item.kind.as_str().to_owned(),
                        code: item.code,
                        quantity: item.quantity,
                        price: price.map(|price| MoneyDto {
                            amount: i64::from(price.amount),
                            currency: price.currency.clone(),
                        }),
                        grandfathered: price.is_some_and(|price| !price.active),
                    }
                })
                .collect();
            Some(AdminSubscriptionDto {
                stripe_url: stripe_link(
                    state,
                    DashboardPage::Subscription,
                    Some(&subscription.stripe_subscription_id),
                ),
                status: subscription.status,
                interval: BillingInterval::parse(&subscription.billing_interval)
                    .unwrap_or(BillingInterval::Month),
                current_period_end: subscription.current_period_end,
                cancel_at_period_end: subscription.cancel_at_period_end,
                held_plan_code: subscription.held_plan_code,
                items,
            })
        }
        None => None,
    };

    let support_access = support::grant_in_force(state, workspace.id).await?;
    let channels = db::chat::channels_of_workspace(&state.db, workspace.id).await?;
    let ids: Vec<Uuid> = channels.iter().map(|channel| channel.id).collect();
    let links = db::chat::links_of_channels(&state.db, &ids).await?;
    let chat_channels = channels
        .into_iter()
        .map(|channel| AdminChatChannelDto {
            links: links
                .iter()
                .filter(|link| link.chat_channel_id == channel.id)
                .map(|link| AdminChatLinkDto {
                    switcher_id: link.switcher_id,
                    switcher_name: link.switcher_name.clone(),
                    workspace_id: link.workspace_id,
                    workspace_name: link.workspace_name.clone(),
                })
                .collect(),
            id: channel.id,
            platform: channel.platform.as_str().to_owned(),
            login: channel.login,
            display_name: channel.display_name,
            status: match channel.status {
                db::chat::GrantStatus::Active => "active",
                db::chat::GrantStatus::Revoked => "revoked",
            }
            .to_owned(),
        })
        .collect();

    Ok(AdminWorkspaceDetailDto {
        support_access,
        chat_channels,
        workspace: workspace_dto(state, workspace),
        members: members
            .into_iter()
            .map(|member| AdminMemberDto {
                user_id: member.user_id,
                username: member.username,
                provider: member.provider,
                role: member.role.into(),
            })
            .collect(),
        switchers: switchers
            .into_iter()
            .map(|switcher| AdminSwitcherDto {
                id: switcher.id,
                name: switcher.name,
                last_seen_at: switcher.last_seen_at,
                stream_server_count: switcher.stream_server_count,
                paused: switcher.paused,
            })
            .collect(),
        subscription,
        addons: addons
            .into_iter()
            .map(|status| WorkspaceAddonDto {
                quantity: status.quantity(),
                billed: status.billed,
                held_until: status.held.and(status.held_until),
                code: status.code,
            })
            .collect(),
    })
}

/// One workspace: members, switchers and billing.
#[utoipa::path(get, path = "/admin/workspaces/{workspace_id}", tag = "admin",
    params(("workspace_id" = Uuid, Path)),
    responses((status = OK, body = AdminWorkspaceDetailDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_get_workspace(
    State(state): State<ApiState>,
    _staff: Staff,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<AdminWorkspaceDetailDto>> {
    Ok(Json(workspace_detail(&state, workspace_id).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetPlan {
    pub code: String,
}

/// Set a workspace's plan directly (a comp). Meant for workspaces without a
/// subscription: the next Stripe event for one recomputes its plan.
#[utoipa::path(put, path = "/admin/workspaces/{workspace_id}/plan", tag = "admin",
    params(("workspace_id" = Uuid, Path)),
    request_body = SetPlan,
    responses((status = OK, body = AdminWorkspaceDetailDto), (status = BAD_REQUEST, body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_set_workspace_plan(
    State(state): State<ApiState>,
    Staff(me): Staff,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<SetPlan>,
) -> ApiResult<Json<AdminWorkspaceDetailDto>> {
    let plan = db::plans::get_by_code(&state.db, &request.code)
        .await?
        .ok_or_else(|| ApiError::bad_request("invalid_plan", "Unknown plan"))?;
    let workspace = db::workspaces::set_plan(&state.db, workspace_id, &plan.code)
        .await?
        .ok_or_else(|| ApiError::not_found("Workspace not found"))?;
    admin_actions::record(
        &state,
        &me,
        admin_actions::Action {
            action: "set_plan",
            route: "/admin/workspaces/{workspace_id}/plan",
            method: "PUT",
            workspace: Some((workspace_id, &workspace.name)),
            details: serde_json::json!({ "plan": plan.code }),
            ..admin_actions::Action::default()
        },
    )
    .await?;
    // Switchers over (or back under) the new limit pause or resume.
    state.notify_workspace(workspace_id).await;
    Ok(Json(workspace_detail(&state, workspace_id).await?))
}

// --- Products --------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminPriceDto {
    pub interval: BillingInterval,
    pub amount: i64,
    pub currency: String,
    pub stripe_price_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminPlanDto {
    pub code: String,
    pub name: String,
    pub description: String,
    pub is_active: bool,
    pub position: i32,
    pub max_switchers: i32,
    /// Per switcher.
    pub max_stream_servers: i32,
    pub max_members: i32,
    pub history_days: i32,
    /// Workspaces on it may connect their own chat bot.
    pub custom_chat_bot: bool,
    /// What new subscribers pay (at most one per interval).
    pub prices: Vec<AdminPriceDto>,
    /// Earlier prices, still paid by the subscriptions on them.
    pub archived_prices: i64,
    /// Workspaces on this plan (paying or comped).
    pub workspaces: i64,
    /// Workspaces with a live subscription for it.
    pub subscribers: i64,
    pub stripe_product_url: Option<String>,
    /// Why it can't be removed; `null` when it can (nothing depends on it).
    pub removal_blocked_by: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminAddonDto {
    pub code: String,
    pub name: String,
    pub description: String,
    pub unit: String,
    pub max_quantity: i32,
    /// As set here; the Plan page shows `available` without a price as coming soon.
    pub status: AddonVisibility,
    pub position: i32,
    pub prices: Vec<AdminPriceDto>,
    pub archived_prices: i64,
    pub subscribers: i64,
    pub stripe_product_url: Option<String>,
    /// Why it can't be removed; `null` when it can (nothing depends on it).
    pub removal_blocked_by: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminProductsDto {
    pub billing_enabled: bool,
    /// The currency prices use (Checkout can't mix currencies in one subscription).
    pub currency: Option<String>,
    pub plans: Vec<AdminPlanDto>,
    pub addons: Vec<AdminAddonDto>,
}

fn price_dtos(
    prices: &[db::prices::Price],
    kind: ItemKind,
    code: &str,
) -> (Vec<AdminPriceDto>, i64) {
    let mine = prices
        .iter()
        .filter(|price| price.kind == kind && price.code == code);
    let mut active: Vec<AdminPriceDto> = mine
        .clone()
        .filter(|price| price.active)
        .filter_map(|price| {
            Some(AdminPriceDto {
                interval: BillingInterval::parse(&price.billing_interval)?,
                amount: i64::from(price.amount),
                currency: price.currency.clone(),
                stripe_price_id: price.stripe_price_id.clone(),
            })
        })
        .collect();
    active.sort_by_key(|price| price.interval.as_str() == "year");
    let archived = mine.filter(|price| !price.active).count();
    (active, i64::try_from(archived).unwrap_or(0))
}

async fn products_dto(state: &ApiState) -> ApiResult<AdminProductsDto> {
    let prices = db::prices::all(&state.db).await?;
    let plans = db::plans::list_all(&state.db).await?;
    let addons = db::billing::list_addons(&state.db).await?;
    let subscribers = db::admin::subscribers(&state.db).await?;
    let on_plan = db::admin::overview(&state.db).await?.plans;
    let currencies = db::prices::currencies_in_use(&state.db).await?;
    let mut blocked = std::collections::HashMap::new();
    for (kind, code) in plans
        .iter()
        .map(|plan| (ItemKind::Plan, plan.code.clone()))
        .chain(
            addons
                .iter()
                .map(|addon| (ItemKind::Addon, addon.code.clone())),
        )
    {
        let reason = removal_blocked_by(state, kind, &code).await?;
        blocked.insert((kind, code), reason);
    }
    let blocked_by =
        |kind: ItemKind, code: &str| blocked.get(&(kind, code.to_owned())).cloned().flatten();
    let count = |kind: ItemKind, code: &str| {
        subscribers
            .iter()
            .find(|row| row.kind == kind.as_str() && row.code == code)
            .map_or(0, |row| row.workspaces)
    };

    Ok(AdminProductsDto {
        billing_enabled: state.billing.is_some(),
        currency: currencies.into_iter().next(),
        plans: plans
            .into_iter()
            .map(|plan| {
                let (active, archived) = price_dtos(&prices, ItemKind::Plan, &plan.code);
                AdminPlanDto {
                    removal_blocked_by: blocked_by(ItemKind::Plan, &plan.code),
                    workspaces: on_plan
                        .iter()
                        .find(|row| row.code == plan.code)
                        .map_or(0, |row| row.workspaces),
                    subscribers: count(ItemKind::Plan, &plan.code),
                    stripe_product_url: stripe_link(
                        state,
                        DashboardPage::Product,
                        plan.stripe_product_id.as_deref(),
                    ),
                    prices: active,
                    archived_prices: archived,
                    code: plan.code,
                    name: plan.name,
                    description: plan.description,
                    is_active: plan.is_active,
                    position: plan.position,
                    max_switchers: plan.max_switchers,
                    max_stream_servers: plan.max_stream_servers,
                    max_members: plan.max_workspace_members,
                    history_days: plan.history_days,
                    custom_chat_bot: plan.custom_chat_bot,
                }
            })
            .collect(),
        addons: addons
            .into_iter()
            .map(|addon| {
                let (active, archived) = price_dtos(&prices, ItemKind::Addon, &addon.code);
                AdminAddonDto {
                    removal_blocked_by: blocked_by(ItemKind::Addon, &addon.code),
                    subscribers: count(ItemKind::Addon, &addon.code),
                    stripe_product_url: stripe_link(
                        state,
                        DashboardPage::Product,
                        addon.stripe_product_id.as_deref(),
                    ),
                    prices: active,
                    archived_prices: archived,
                    code: addon.code,
                    name: addon.name,
                    description: addon.description,
                    unit: addon.unit,
                    max_quantity: addon.max_quantity,
                    status: addon.status.into(),
                    position: addon.position,
                }
            })
            .collect(),
    })
}

/// Every plan and add-on, offered or not, with prices and subscriber counts.
#[utoipa::path(get, path = "/admin/products", tag = "admin",
    responses((status = OK, body = AdminProductsDto), (status = FORBIDDEN, body = ErrorBody)))]
async fn admin_products(
    State(state): State<ApiState>,
    _staff: Staff,
) -> ApiResult<Json<AdminProductsDto>> {
    Ok(Json(products_dto(&state).await?))
}

/// Lowercase letters, digits and single dashes: codes end up in URLs and config.
fn check_code(code: &str) -> ApiResult<()> {
    let valid = (2..=30).contains(&code.len())
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !code.starts_with('-')
        && !code.ends_with('-')
        && !code.contains("--");
    if valid {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "invalid_code",
            "Use 2–30 lowercase letters, digits and dashes.",
        ))
    }
}

fn check_name(name: &str) -> ApiResult<()> {
    if name.trim().is_empty() || name.chars().count() > 60 {
        return Err(ApiError::bad_request(
            "invalid_name",
            "Give it a name of up to 60 characters.",
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PlanInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub is_active: bool,
    #[serde(default)]
    pub position: i32,
    pub max_switchers: i32,
    /// Per switcher.
    pub max_stream_servers: i32,
    pub max_members: i32,
    pub history_days: i32,
    #[serde(default)]
    pub custom_chat_bot: bool,
}

impl PlanInput {
    fn check(&self) -> ApiResult<()> {
        check_name(&self.name)?;
        if [
            self.max_switchers,
            self.max_stream_servers,
            self.max_members,
            self.history_days,
        ]
        .iter()
        .any(|&limit| limit < 1)
        {
            return Err(ApiError::bad_request(
                "invalid_limits",
                "Switchers, stream servers, members and days of history must be at least 1.",
            ));
        }
        Ok(())
    }

    fn fields(&self) -> db::plans::PlanFields<'_> {
        db::plans::PlanFields {
            name: self.name.trim(),
            description: self.description.trim(),
            is_active: self.is_active,
            position: self.position,
            max_switchers: self.max_switchers,
            max_stream_servers: self.max_stream_servers,
            max_workspace_members: self.max_members,
            history_days: self.history_days,
            custom_chat_bot: self.custom_chat_bot,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreatePlan {
    pub code: String,
    #[serde(flatten)]
    #[schema(inline)]
    pub plan: PlanInput,
}

/// Create a plan. Its Stripe product is made when its first price is set.
#[utoipa::path(post, path = "/admin/plans", tag = "admin",
    request_body = CreatePlan,
    responses((status = OK, body = AdminProductsDto), (status = BAD_REQUEST, body = ErrorBody),
        (status = CONFLICT, body = ErrorBody)))]
async fn admin_create_plan(
    State(state): State<ApiState>,
    _staff: Staff,
    Json(request): Json<CreatePlan>,
) -> ApiResult<Json<AdminProductsDto>> {
    check_code(&request.code)?;
    request.plan.check()?;
    // Its Stripe product is made with its first price (or adopted from a linked one).
    db::plans::create(&state.db, &request.code, &request.plan.fields()).await?;
    Ok(Json(products_dto(&state).await?))
}

/// Edit a plan. Its limits apply to every workspace on it right away.
#[utoipa::path(put, path = "/admin/plans/{code}", tag = "admin",
    params(("code" = String, Path)),
    request_body = PlanInput,
    responses((status = OK, body = AdminProductsDto), (status = BAD_REQUEST, body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_update_plan(
    State(state): State<ApiState>,
    _staff: Staff,
    Path(code): Path<String>,
    Json(request): Json<PlanInput>,
) -> ApiResult<Json<AdminProductsDto>> {
    request.check()?;
    if code == db::plans::FREE && !request.is_active {
        return Err(ApiError::bad_request(
            "free_plan",
            "The Free plan is where workspaces land without a subscription; it stays offered.",
        ));
    }
    let before = db::plans::get_by_code(&state.db, &code)
        .await?
        .ok_or_else(|| ApiError::not_found("Plan not found"))?;
    let plan = db::plans::update(&state.db, &code, &request.fields())
        .await?
        .ok_or_else(|| ApiError::not_found("Plan not found"))?;

    if let (Some(billing), Some(product)) = (&state.billing, &plan.stripe_product_id) {
        billing
            .stripe
            .update_product(product, &plan.name, &plan.description, None, plan.is_active)
            .await
            .map_err(stripe_error)?;
    }
    // A new switcher limit pauses or resumes switchers in every workspace on the plan.
    if before.max_switchers != plan.max_switchers {
        for workspace_id in db::plans::workspace_ids(&state.db, &code).await? {
            state.notify_workspace(workspace_id).await;
        }
    }
    // Which bot speaks for its workspaces (theirs or the shared one).
    if before.custom_chat_bot != plan.custom_chat_bot {
        state.notify_chat().await;
    }
    Ok(Json(products_dto(&state).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddonInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// What one unit is called, e.g. `slot`.
    pub unit: String,
    pub max_quantity: i32,
    pub status: AddonVisibility,
    #[serde(default)]
    pub position: i32,
}

impl AddonInput {
    fn check(&self) -> ApiResult<()> {
        check_name(&self.name)?;
        let unit = self.unit.trim();
        if unit.is_empty() || unit.chars().count() > 20 {
            return Err(ApiError::bad_request(
                "invalid_unit",
                "Name one unit in up to 20 characters, e.g. slot.",
            ));
        }
        if !(1..=MAX_ADDON_QUANTITY).contains(&self.max_quantity) {
            return Err(ApiError::bad_request(
                "invalid_limits",
                format!("The most one workspace can buy is between 1 and {MAX_ADDON_QUANTITY}."),
            ));
        }
        Ok(())
    }

    fn fields(&self) -> db::billing::AddonFields<'_> {
        db::billing::AddonFields {
            name: self.name.trim(),
            description: self.description.trim(),
            unit: self.unit.trim(),
            max_quantity: self.max_quantity,
            status: self.status.into(),
            position: self.position,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateAddon {
    pub code: String,
    #[serde(flatten)]
    #[schema(inline)]
    pub addon: AddonInput,
}

/// Create an add-on. Its Stripe product is made when its first price is set.
#[utoipa::path(post, path = "/admin/addons", tag = "admin",
    request_body = CreateAddon,
    responses((status = OK, body = AdminProductsDto), (status = BAD_REQUEST, body = ErrorBody),
        (status = CONFLICT, body = ErrorBody)))]
async fn admin_create_addon(
    State(state): State<ApiState>,
    _staff: Staff,
    Json(request): Json<CreateAddon>,
) -> ApiResult<Json<AdminProductsDto>> {
    check_code(&request.code)?;
    request.addon.check()?;
    db::billing::create_addon(&state.db, &request.code, &request.addon.fields()).await?;
    Ok(Json(products_dto(&state).await?))
}

/// Edit an add-on. A lower maximum applies to changes from now on; nobody loses
/// what they have.
#[utoipa::path(put, path = "/admin/addons/{code}", tag = "admin",
    params(("code" = String, Path)),
    request_body = AddonInput,
    responses((status = OK, body = AdminProductsDto), (status = BAD_REQUEST, body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_update_addon(
    State(state): State<ApiState>,
    _staff: Staff,
    Path(code): Path<String>,
    Json(request): Json<AddonInput>,
) -> ApiResult<Json<AdminProductsDto>> {
    request.check()?;
    let addon = db::billing::update_addon(&state.db, &code, &request.fields())
        .await?
        .ok_or_else(|| ApiError::not_found("Add-on not found"))?;
    if let (Some(billing), Some(product)) = (&state.billing, &addon.stripe_product_id) {
        billing
            .stripe
            .update_product(
                product,
                &addon.name,
                &addon.description,
                Some(&addon.unit),
                true,
            )
            .await
            .map_err(stripe_error)?;
    }
    Ok(Json(products_dto(&state).await?))
}

/// The Stripe product for a plan or add-on, created on first need. `None`
/// while billing is off.
async fn ensure_product(state: &ApiState, kind: ItemKind, code: &str) -> ApiResult<Option<String>> {
    let Some(billing) = &state.billing else {
        return Ok(None);
    };
    let (existing, name, description, unit) = match kind {
        ItemKind::Plan => {
            let plan = db::plans::get_by_code(&state.db, code)
                .await?
                .ok_or_else(|| ApiError::not_found("Plan not found"))?;
            (plan.stripe_product_id, plan.name, plan.description, None)
        }
        ItemKind::Addon => {
            let addon = db::billing::get_addon(&state.db, code)
                .await?
                .ok_or_else(|| ApiError::not_found("Add-on not found"))?;
            (
                addon.stripe_product_id,
                addon.name,
                addon.description,
                Some(addon.unit),
            )
        }
    };
    if let Some(existing) = existing {
        return Ok(Some(existing));
    }
    let product = billing
        .stripe
        .create_product(&name, &description, unit.as_deref(), kind.as_str(), code)
        .await
        .map_err(stripe_error)?;
    match kind {
        ItemKind::Plan => db::plans::set_stripe_product(&state.db, code, &product.id).await?,
        ItemKind::Addon => {
            db::billing::set_addon_stripe_product(&state.db, code, &product.id).await?;
        }
    }
    Ok(Some(product.id))
}

fn parse_kind(kind: &str) -> ApiResult<ItemKind> {
    ItemKind::parse(kind).ok_or_else(|| ApiError::not_found("No such product kind"))
}

fn parse_interval(interval: &str) -> ApiResult<BillingInterval> {
    BillingInterval::parse(interval).ok_or_else(|| ApiError::not_found("No such billing period"))
}

/// A priced product: an existing paid plan or add-on.
async fn priced_product(state: &ApiState, kind: ItemKind, code: &str) -> ApiResult<()> {
    match kind {
        ItemKind::Plan if code == db::plans::FREE => Err(ApiError::bad_request(
            "free_plan",
            "The Free plan has no price.",
        )),
        ItemKind::Plan => db::plans::get_by_code(&state.db, code)
            .await?
            .map(|_| ())
            .ok_or_else(|| ApiError::not_found("Plan not found")),
        ItemKind::Addon => db::billing::get_addon(&state.db, code)
            .await?
            .map(|_| ())
            .ok_or_else(|| ApiError::not_found("Add-on not found")),
    }
}

/// Checkout can't mix currencies in one subscription, so every active price
/// uses one.
async fn check_currency(state: &ApiState, currency: &str) -> ApiResult<()> {
    let valid = currency.len() == 3 && currency.bytes().all(|byte| byte.is_ascii_lowercase());
    if !valid {
        return Err(ApiError::bad_request(
            "invalid_currency",
            "Use a three-letter currency code, e.g. eur.",
        ));
    }
    let in_use = db::prices::currencies_in_use(&state.db).await?;
    if in_use.iter().any(|used| used != currency) {
        return Err(ApiError::bad_request(
            "currency_mismatch",
            format!(
                "Prices are in {}. Use the same currency: one subscription can't mix them.",
                in_use.join(", ").to_uppercase()
            ),
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SetPriceRequest {
    /// In the currency's smallest unit (cents); per unit for add-ons.
    pub amount: i32,
    pub currency: String,
}

/// Set what new subscribers pay for a plan or add-on at a billing period. The
/// previous price is archived; subscriptions on it keep it.
#[utoipa::path(put, path = "/admin/products/{kind}/{code}/prices/{interval}", tag = "admin",
    params(("kind" = String, Path, description = "`plan` or `addon`"), ("code" = String, Path),
        ("interval" = String, Path, description = "`month` or `year`")),
    request_body = SetPriceRequest,
    responses((status = OK, body = AdminProductsDto), (status = BAD_REQUEST, body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, body = ErrorBody)))]
async fn admin_set_price(
    State(state): State<ApiState>,
    Staff(me): Staff,
    Path(path): Path<(String, String, String)>,
    Json(request): Json<SetPriceRequest>,
) -> ApiResult<Json<AdminProductsDto>> {
    let (kind, code, interval) = path;
    let (kind, interval) = (parse_kind(&kind)?, parse_interval(&interval)?);
    let billing = state.billing.clone().ok_or_else(billing_disabled)?;
    priced_product(&state, kind, &code).await?;
    if request.amount < 1 {
        return Err(ApiError::bad_request(
            "invalid_amount",
            "The price must be above zero.",
        ));
    }
    let currency = request.currency.trim().to_lowercase();
    check_currency(&state, &currency).await?;

    let product = ensure_product(&state, kind, &code)
        .await?
        .ok_or_else(billing_disabled)?;
    let price = billing
        .stripe
        .create_price(&product, request.amount, &currency, interval.as_str())
        .await
        .map_err(stripe_error)?;
    let replaced = db::prices::set_active(
        &state.db,
        kind,
        &code,
        interval.as_str(),
        request.amount,
        &currency,
        &price.id,
    )
    .await?;
    if let Some(replaced) = replaced
        && let Err(error) = billing
            .stripe
            .archive_price(&replaced.stripe_price_id)
            .await
    {
        // Not sold any more either way: our catalog decides what's offered.
        tracing::warn!(%error, price = %replaced.stripe_price_id, "Couldn't archive the old price in Stripe");
    }
    tracing::info!(kind = kind.as_str(), %code, interval = interval.as_str(), amount = request.amount, by = %me.id(), "Price set");
    Ok(Json(products_dto(&state).await?))
}

/// Stop selling a plan or add-on at a billing period. Subscriptions on the
/// price keep it.
#[utoipa::path(delete, path = "/admin/products/{kind}/{code}/prices/{interval}", tag = "admin",
    params(("kind" = String, Path), ("code" = String, Path), ("interval" = String, Path)),
    responses((status = OK, body = AdminProductsDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_archive_price(
    State(state): State<ApiState>,
    _staff: Staff,
    Path(path): Path<(String, String, String)>,
) -> ApiResult<Json<AdminProductsDto>> {
    let (kind, code, interval) = path;
    let (kind, interval) = (parse_kind(&kind)?, parse_interval(&interval)?);
    let archived = db::prices::archive(&state.db, kind, &code, interval.as_str())
        .await?
        .ok_or_else(|| ApiError::not_found("It isn't sold with that billing period"))?;
    if let Some(billing) = &state.billing
        && let Err(error) = billing
            .stripe
            .archive_price(&archived.stripe_price_id)
            .await
    {
        tracing::warn!(%error, price = %archived.stripe_price_id, "Couldn't archive the price in Stripe");
    }
    Ok(Json(products_dto(&state).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LinkPriceRequest {
    /// A recurring price made in the Stripe Dashboard (`price_…`).
    pub stripe_price_id: String,
}

/// Sell a plan or add-on at an existing Stripe price (e.g. one made in the
/// Stripe Dashboard). The previous price is archived here, not in Stripe.
#[utoipa::path(post, path = "/admin/products/{kind}/{code}/prices/{interval}/link", tag = "admin",
    params(("kind" = String, Path), ("code" = String, Path), ("interval" = String, Path)),
    request_body = LinkPriceRequest,
    responses((status = OK, body = AdminProductsDto), (status = BAD_REQUEST, body = ErrorBody),
        (status = SERVICE_UNAVAILABLE, body = ErrorBody)))]
async fn admin_link_price(
    State(state): State<ApiState>,
    _staff: Staff,
    Path(path): Path<(String, String, String)>,
    Json(request): Json<LinkPriceRequest>,
) -> ApiResult<Json<AdminProductsDto>> {
    let (kind, code, interval) = path;
    let (kind, interval) = (parse_kind(&kind)?, parse_interval(&interval)?);
    let billing = state.billing.clone().ok_or_else(billing_disabled)?;
    priced_product(&state, kind, &code).await?;

    let price = billing
        .stripe
        .get_price(request.stripe_price_id.trim())
        .await
        .map_err(stripe_error)?;
    let recurring = price
        .recurring
        .as_ref()
        .map(|recurring| recurring.interval.as_str());
    if recurring != Some(interval.as_str()) {
        return Err(ApiError::bad_request(
            "wrong_interval",
            format!("That price isn't billed every {}.", interval.as_str()),
        ));
    }
    let amount = price
        .unit_amount
        .and_then(|amount| i32::try_from(amount).ok())
        .filter(|amount| *amount > 0)
        .ok_or_else(|| {
            ApiError::bad_request(
                "invalid_amount",
                "Only flat-rate prices above zero can be linked.",
            )
        })?;
    if !price.active {
        return Err(ApiError::bad_request(
            "price_archived",
            "That price is archived in Stripe.",
        ));
    }
    check_currency(&state, &price.currency).await?;

    // A price belongs to one product: adopt it, or refuse another one's.
    let current = match kind {
        ItemKind::Plan => db::plans::get_by_code(&state.db, &code)
            .await?
            .and_then(|plan| plan.stripe_product_id),
        ItemKind::Addon => db::billing::get_addon(&state.db, &code)
            .await?
            .and_then(|addon| addon.stripe_product_id),
    };
    match (&current, &price.product) {
        (Some(ours), Some(theirs)) if ours != theirs => {
            return Err(ApiError::bad_request(
                "other_product",
                "That price belongs to another Stripe product.",
            ));
        }
        (None, Some(theirs)) => match kind {
            ItemKind::Plan => db::plans::set_stripe_product(&state.db, &code, theirs).await?,
            ItemKind::Addon => {
                db::billing::set_addon_stripe_product(&state.db, &code, theirs).await?;
            }
        },
        _ => {}
    }

    db::prices::set_active(
        &state.db,
        kind,
        &code,
        interval.as_str(),
        amount,
        &price.currency,
        &price.id,
    )
    .await?;
    Ok(Json(products_dto(&state).await?))
}

fn billing_disabled() -> ApiError {
    ApiError::new(
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        "billing_disabled",
        "Set up Stripe first: prices are created there.",
    )
}

/// Why a plan or add-on can't be removed, in words; `None` when nothing depends on it.
async fn removal_blocked_by(
    state: &ApiState,
    kind: ItemKind,
    code: &str,
) -> ApiResult<Option<String>> {
    if kind == ItemKind::Plan && code == db::plans::FREE {
        return Ok(Some(
            "Free is where workspaces without a subscription land.".into(),
        ));
    }
    let plural = |count: i64, one: &str, many: &str| {
        format!("{count} {}", if count == 1 { one } else { many })
    };
    Ok(match db::admin::in_use(&state.db, kind, code).await? {
        None => None,
        Some(db::admin::InUse::Workspaces(count)) => Some(format!(
            "{} on it. Move them to another plan first.",
            plural(count, "workspace is", "workspaces are")
        )),
        Some(db::admin::InUse::Subscriptions(count)) => Some(format!(
            "{} it. It can go once they've ended.",
            plural(count, "subscription pays for", "subscriptions pay for")
        )),
    })
}

/// Remove a plan nobody is on and nobody pays for. Its Stripe product and
/// prices are archived.
#[utoipa::path(delete, path = "/admin/plans/{code}", tag = "admin",
    params(("code" = String, Path)),
    responses((status = OK, body = AdminProductsDto), (status = NOT_FOUND, body = ErrorBody),
        (status = CONFLICT, body = ErrorBody, description = "Still in use")))]
async fn admin_delete_plan(
    State(state): State<ApiState>,
    Staff(me): Staff,
    Path(code): Path<String>,
) -> ApiResult<Json<AdminProductsDto>> {
    let plan = db::plans::get_by_code(&state.db, &code)
        .await?
        .ok_or_else(|| ApiError::not_found("Plan not found"))?;
    remove_product(
        &state,
        ItemKind::Plan,
        &code,
        plan.stripe_product_id.as_deref(),
    )
    .await?;
    tracing::info!(%code, by = %me.id(), "Plan removed");
    Ok(Json(products_dto(&state).await?))
}

/// Remove an add-on nobody pays for. Its Stripe product and prices are archived.
#[utoipa::path(delete, path = "/admin/addons/{code}", tag = "admin",
    params(("code" = String, Path)),
    responses((status = OK, body = AdminProductsDto), (status = NOT_FOUND, body = ErrorBody),
        (status = CONFLICT, body = ErrorBody, description = "Still in use")))]
async fn admin_delete_addon(
    State(state): State<ApiState>,
    Staff(me): Staff,
    Path(code): Path<String>,
) -> ApiResult<Json<AdminProductsDto>> {
    let addon = db::billing::get_addon(&state.db, &code)
        .await?
        .ok_or_else(|| ApiError::not_found("Add-on not found"))?;
    remove_product(
        &state,
        ItemKind::Addon,
        &code,
        addon.stripe_product_id.as_deref(),
    )
    .await?;
    tracing::info!(%code, by = %me.id(), "Add-on removed");
    Ok(Json(products_dto(&state).await?))
}

async fn remove_product(
    state: &ApiState,
    kind: ItemKind,
    code: &str,
    stripe_product: Option<&str>,
) -> ApiResult<()> {
    if let Some(reason) = removal_blocked_by(state, kind, code).await? {
        return Err(ApiError::conflict("in_use", reason));
    }
    let active_prices = match db::admin::remove_product(&state.db, kind, code).await? {
        Ok(prices) => prices,
        // Something started using it since the check above.
        Err(_) => {
            return Err(ApiError::conflict(
                "in_use",
                "It's in use now. Reload and try again.",
            ));
        }
    };
    // Gone from our catalog either way; tidy Stripe up as well as we can.
    if let Some(billing) = &state.billing {
        for price in active_prices {
            if let Err(error) = billing.stripe.archive_price(&price).await {
                tracing::warn!(%error, %price, "Couldn't archive a removed product's price in Stripe");
            }
        }
        if let Some(product) = stripe_product
            && let Err(error) = billing.stripe.archive_product(product).await
        {
            tracing::warn!(%error, %product, "Couldn't archive a removed product in Stripe");
        }
    }
    Ok(())
}
