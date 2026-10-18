//! Organizations (`db::organizations`): companies that run client workspaces for
//! their customers. The team lists and creates clients, hands each customer an
//! invite link (the ordinary workspace invite, role admin: the customer never
//! gets billing, since the organization pays from its pool) and manages its own
//! team. Staff create organizations and set their limits (`admin_organizations.rs`).

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{Duration, Utc};
use db::organizations::AcceptOutcome;
use futures_util::StreamExt;
use live::SwitcherStatus;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{
        CurrentUser, OrgManage, OrgMembers, OrgView, OrganizationAuthorized, user_display_name,
    },
    routes::{
        ApiError, ApiResult, ErrorBody,
        organization_audit::{record, record_team},
        workspaces::SuspensionDto,
    },
    validate,
};

/// The most switchers one client may be allocated.
const MAX_ALLOCATION: i32 = 10_000;
/// Clients per page of the list.
const CLIENTS_PER_PAGE: i64 = 25;
pub(crate) const DEFAULT_EXPIRY_DAYS: i64 = 7;
pub(crate) const MAX_EXPIRY_DAYS: i64 = 30;

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_organizations))
        .routes(routes!(get_organization_by_slug))
        .routes(routes!(get_organization))
        .routes(routes!(update_organization_settings))
        .routes(routes!(list_clients, create_client))
        .routes(routes!(update_client, delete_client))
        .routes(routes!(list_team))
        .routes(routes!(update_team_member, remove_team_member))
        .routes(routes!(list_team_invites, create_team_invite))
        .routes(routes!(revoke_team_invite))
        .routes(routes!(preview_team_invite))
        .routes(routes!(accept_team_invite))
}

/// A role on an organization's team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrganizationRole {
    /// Runs the clients' switchers and settings.
    Member,
    /// Also creates and deletes clients and manages the team.
    Admin,
}

impl From<db::organizations::OrganizationRole> for OrganizationRole {
    fn from(role: db::organizations::OrganizationRole) -> Self {
        use db::organizations::OrganizationRole as Db;
        match role {
            Db::Member => Self::Member,
            Db::Admin => Self::Admin,
        }
    }
}

impl From<OrganizationRole> for db::organizations::OrganizationRole {
    fn from(role: OrganizationRole) -> Self {
        match role {
            OrganizationRole::Member => Self::Member,
            OrganizationRole::Admin => Self::Admin,
        }
    }
}

/// What staff granted an organization.
#[derive(Debug, Serialize, ToSchema)]
pub struct OrganizationLimitsDto {
    /// The pool: switchers in all its client workspaces together.
    pub max_switchers: i32,
    /// Per switcher.
    pub max_stream_servers: i32,
    /// Per client workspace (the customers who joined; the team isn't counted).
    pub max_members: i32,
    /// Days of stream history kept.
    pub history_days: i32,
}

/// What the organization decides itself.
#[derive(Debug, Serialize, ToSchema)]
pub struct OrganizationSettingsDto {
    /// Its customers may connect a chat bot of their own, which speaks in their
    /// channels instead of the organization's bot.
    pub custom_chat_bot: bool,
}

/// How much of an organization is in use.
#[derive(Debug, Serialize, ToSchema)]
pub struct OrganizationUsageDto {
    pub clients: i64,
    pub switchers: i64,
    pub team: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OrganizationDto {
    pub id: Uuid,
    pub name: String,
    pub url_slug: String,
    /// The current user's role; `null` for staff who aren't on the team.
    pub role: Option<OrganizationRole>,
    pub limits: OrganizationLimitsDto,
    pub settings: OrganizationSettingsDto,
    pub usage: OrganizationUsageDto,
    /// Suspended by NOALBS: every client's switchers paused and chat quiet.
    pub suspension: Option<SuspensionDto>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<db::organizations::OrganizationSummary> for OrganizationDto {
    fn from(organization: db::organizations::OrganizationSummary) -> Self {
        Self {
            id: organization.id,
            name: organization.name,
            url_slug: organization.url_slug,
            role: organization.role.map(Into::into),
            limits: OrganizationLimitsDto {
                max_switchers: organization.max_switchers,
                max_stream_servers: organization.max_stream_servers,
                max_members: organization.max_workspace_members,
                history_days: organization.history_days,
            },
            settings: OrganizationSettingsDto {
                custom_chat_bot: organization.custom_chat_bot,
            },
            usage: OrganizationUsageDto {
                clients: organization.workspace_count,
                switchers: organization.switcher_count,
                team: organization.member_count,
            },
            suspension: organization.suspended_at.map(|since| SuspensionDto {
                since,
                reason: organization.suspended_reason.unwrap_or_default(),
            }),
            created_at: organization.created_at,
        }
    }
}

pub(crate) async fn summary(
    state: &ApiState,
    organization_id: Uuid,
    user_id: Uuid,
) -> ApiResult<OrganizationDto> {
    db::organizations::get_summary(&state.db, organization_id, user_id)
        .await?
        .map(Into::into)
        .ok_or_else(|| ApiError::not_found("Organization not found"))
}

/// List the organizations the current user is on the team of.
#[utoipa::path(get, path = "/organizations", tag = "organizations",
    responses((status = OK, body = Vec<OrganizationDto>), (status = UNAUTHORIZED, body = ErrorBody)))]
async fn list_organizations(
    State(state): State<ApiState>,
    current_user: CurrentUser,
) -> ApiResult<Json<Vec<OrganizationDto>>> {
    let organizations = db::organizations::list_for_user(&state.db, current_user.id()).await?;

    Ok(Json(organizations.into_iter().map(Into::into).collect()))
}

/// An organization by its URL slug, for the dashboard's `/orgs/{slug}` pages.
/// Only its team can see it; anyone else gets 404.
#[utoipa::path(get, path = "/organizations/by-slug/{url_slug}", tag = "organizations",
    params(("url_slug" = String, Path)),
    responses((status = OK, body = OrganizationDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn get_organization_by_slug(
    State(state): State<ApiState>,
    current_user: CurrentUser,
    Path(url_slug): Path<String>,
) -> ApiResult<Json<OrganizationDto>> {
    db::organizations::get_summary_by_slug(&state.db, &url_slug, current_user.id())
        .await?
        .filter(|organization| organization.role.is_some())
        .map(|organization| Json(organization.into()))
        .ok_or_else(|| ApiError::not_found("Organization not found"))
}

/// Get an organization.
#[utoipa::path(get, path = "/organizations/{organization_id}", tag = "organizations",
    params(("organization_id" = Uuid, Path)),
    responses((status = OK, body = OrganizationDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn get_organization(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgView>,
) -> ApiResult<Json<OrganizationDto>> {
    let access = authorized.into_access();

    Ok(Json(
        summary(&state, access.organization_id, access.user.id()).await?,
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateOrganizationSettings {
    pub custom_chat_bot: bool,
}

/// Change what the organization decides itself: whether its customers may connect
/// a chat bot of their own.
#[utoipa::path(put, path = "/organizations/{organization_id}/settings", tag = "organizations",
    params(("organization_id" = Uuid, Path)),
    request_body = UpdateOrganizationSettings,
    responses((status = OK, body = OrganizationDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn update_organization_settings(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgManage>,
    Json(request): Json<UpdateOrganizationSettings>,
) -> ApiResult<Json<OrganizationDto>> {
    let access = authorized.into_access();
    db::organizations::set_custom_chat_bot(
        &state.db,
        access.organization_id,
        request.custom_chat_bot,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("Organization not found"))?;
    // Which bot speaks in the customers' channels may change.
    state.notify_chat().await;
    record_team(
        &state,
        &access,
        "settings_changed",
        None,
        serde_json::json!({ "custom_chat_bot": request.custom_chat_bot }),
    )
    .await?;

    Ok(Json(
        summary(&state, access.organization_id, access.user.id()).await?,
    ))
}

// --- Clients ----------------------------------------------------------------

/// A client workspace as its organization sees it.
#[derive(Debug, Serialize, ToSchema)]
pub struct ClientDto {
    pub id: Uuid,
    pub name: String,
    pub url_slug: String,
    /// Most switchers this client may have; `null` = only the pool limits it.
    pub switcher_allocation: Option<i32>,
    pub switchers: i64,
    /// Customers who joined through an invite (the team isn't counted).
    pub customers: i64,
    /// Some switcher has its OBS connected.
    pub obs_connected: bool,
    /// Some switcher is live.
    pub streaming: bool,
    /// Suspended by NOALBS.
    pub suspended: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl ClientDto {
    pub(crate) fn new(
        client: db::organizations::Client,
        obs_connected: bool,
        streaming: bool,
    ) -> Self {
        Self {
            id: client.id,
            name: client.name,
            url_slug: client.url_slug,
            switcher_allocation: client.switcher_allocation,
            switchers: client.switcher_count,
            customers: client.member_count,
            obs_connected,
            streaming,
            suspended: client.suspended_at.is_some(),
            created_at: client.created_at,
        }
    }
}

/// A page of an organization's clients.
#[derive(Debug, Serialize, ToSchema)]
pub struct ClientPage {
    pub items: Vec<ClientDto>,
    /// Clients matching the search and filter, in all pages.
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

/// Whether a customer joined a client.
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClientCustomer {
    /// At least one customer joined.
    Joined,
    /// Nobody joined yet: only the organization's team is in it.
    Unclaimed,
}

/// How the clients are ordered.
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClientSort {
    Newest,
    Name,
}

/// What a client is doing now (what the dot in the list shows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClientState {
    /// A switcher is streaming.
    Live,
    /// OBS is connected, nothing streams.
    Online,
    /// Nothing is connected.
    Offline,
    /// Suspended by NOALBS.
    Suspended,
}

impl ClientDto {
    pub(crate) fn state(&self) -> ClientState {
        if self.suspended {
            ClientState::Suspended
        } else if self.streaming {
            ClientState::Live
        } else if self.obs_connected {
            ClientState::Online
        } else {
            ClientState::Offline
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct ClientsQuery {
    /// Search text (name, slug or ID).
    pub q: Option<String>,
    /// 1-based.
    pub page: Option<i64>,
    #[param(inline)]
    pub customer: Option<ClientCustomer>,
    /// What the client is doing now; reads every client's live state.
    #[param(inline)]
    pub state: Option<ClientState>,
    /// Newest first when left out.
    #[param(inline)]
    pub sort: Option<ClientSort>,
}

/// The most client workspaces one organization may have. Clients aren't counted
/// against the pool, so this is what stops one organization from claiming URLs and
/// rows without end; it also bounds what filtering by live state reads at once.
pub(crate) const MAX_CLIENTS: i64 = 5_000;

/// How many switchers' live status is read at once.
const STATUS_READS: usize = 64;

/// The live status of these switchers (those with none are left out), read a few
/// at a time from the status bucket.
pub(crate) async fn live_statuses(
    state: &ApiState,
    switcher_ids: &[Uuid],
) -> Vec<(Uuid, SwitcherStatus)> {
    futures_util::stream::iter(switcher_ids.iter().copied())
        .map(|id| {
            let state = state.clone();
            async move { state.status_of(id).await.map(|status| (id, status)) }
        })
        .buffer_unordered(STATUS_READS)
        .filter_map(|status| async move { status })
        .collect()
        .await
}

/// Per client workspace: whether some switcher has OBS connected, and whether
/// some switcher is live.
async fn client_live(
    state: &ApiState,
    organization_id: Uuid,
    workspace_ids: &[Uuid],
) -> ApiResult<HashMap<Uuid, (bool, bool)>> {
    let switchers =
        db::organizations::client_switchers(&state.db, organization_id, workspace_ids).await?;
    let owner: HashMap<Uuid, Uuid> = switchers
        .iter()
        .map(|&(workspace_id, switcher_id)| (switcher_id, workspace_id))
        .collect();
    let ids: Vec<Uuid> = owner.keys().copied().collect();

    let mut live: HashMap<Uuid, (bool, bool)> = HashMap::new();
    for (switcher_id, status) in live_statuses(state, &ids).await {
        let entry = live.entry(owner[&switcher_id]).or_default();
        entry.0 |= status.obs_connected;
        entry.1 |= status.streaming;
    }
    Ok(live)
}

/// A page of an organization's clients. `with_live` reads their live state from
/// the status bucket (staff lists skip it); filtering by `state` reads it for every
/// matching client, since it isn't in the database.
pub(crate) async fn client_page(
    state: &ApiState,
    organization_id: Uuid,
    query: &ClientsQuery,
    with_live: bool,
) -> ApiResult<ClientPage> {
    let page = query.page.unwrap_or(1).max(1);
    let filter = db::organizations::ClientFilter {
        search: query.q.as_deref().map(str::trim).filter(|q| !q.is_empty()),
        claimed: query
            .customer
            .map(|customer| matches!(customer, ClientCustomer::Joined)),
        by_name: matches!(query.sort, Some(ClientSort::Name)),
    };
    let by_state = query.state.filter(|_| with_live);
    let (limit, offset) = match by_state {
        Some(_) => (MAX_CLIENTS, 0),
        None => (CLIENTS_PER_PAGE, (page - 1) * CLIENTS_PER_PAGE),
    };
    let (clients, total) =
        db::organizations::list_clients(&state.db, organization_id, filter, limit, offset).await?;
    let live = if with_live {
        let ids: Vec<Uuid> = clients.iter().map(|client| client.id).collect();
        client_live(state, organization_id, &ids).await?
    } else {
        HashMap::new()
    };
    let mut items: Vec<ClientDto> = clients
        .into_iter()
        .map(|client| {
            let (obs_connected, streaming) = live.get(&client.id).copied().unwrap_or_default();
            ClientDto::new(client, obs_connected, streaming)
        })
        .collect();

    let total = match by_state {
        Some(wanted) => {
            items.retain(|client| client.state() == wanted);
            let total = items.len() as i64;
            items = items
                .into_iter()
                .skip(usize::try_from((page - 1) * CLIENTS_PER_PAGE).unwrap_or(usize::MAX))
                .take(CLIENTS_PER_PAGE as usize)
                .collect();
            total
        }
        None => total,
    };

    Ok(ClientPage {
        items,
        total,
        page,
        per_page: CLIENTS_PER_PAGE,
    })
}

/// List a page of an organization's client workspaces, with whether they are live.
#[utoipa::path(get, path = "/organizations/{organization_id}/clients", tag = "organizations",
    params(("organization_id" = Uuid, Path), ClientsQuery),
    responses((status = OK, body = ClientPage), (status = NOT_FOUND, body = ErrorBody)))]
async fn list_clients(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgView>,
    Query(query): Query<ClientsQuery>,
) -> ApiResult<Json<ClientPage>> {
    let access = authorized.into_access();
    Ok(Json(
        client_page(&state, access.organization_id, &query, true).await?,
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateClient {
    pub name: String,
    pub url_slug: String,
    /// Most switchers the client may have; `null` = only the pool limits it.
    pub switcher_allocation: Option<i32>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateClient {
    pub name: String,
    pub switcher_allocation: Option<i32>,
}

pub(crate) fn allocation(value: Option<i32>) -> ApiResult<Option<i32>> {
    if value.is_some_and(|value| !(0..=MAX_ALLOCATION).contains(&value)) {
        return Err(ApiError::bad_request(
            "invalid_allocation",
            format!("Allocate between 0 and {MAX_ALLOCATION} switchers"),
        ));
    }
    Ok(value)
}

async fn client_dto(
    state: &ApiState,
    organization_id: Uuid,
    workspace_id: Uuid,
) -> ApiResult<ClientDto> {
    let client = db::organizations::get_client(&state.db, organization_id, workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Client not found"))?;

    Ok(ClientDto::new(client, false, false))
}

/// Create a client workspace. It has no owner (the organization is) and no plan
/// (the organization's limits apply); invite the customer with an `admin`
/// invite from `/workspaces/{id}/invites` when they should get in.
#[utoipa::path(post, path = "/organizations/{organization_id}/clients", tag = "organizations",
    params(("organization_id" = Uuid, Path)),
    request_body = CreateClient,
    responses(
        (status = CREATED, body = ClientDto),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = FORBIDDEN, description = "`organization_suspended`", body = ErrorBody),
        (status = CONFLICT, description = "`slug_taken`", body = ErrorBody),
    ))]
async fn create_client(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgManage>,
    Json(request): Json<CreateClient>,
) -> ApiResult<(StatusCode, Json<ClientDto>)> {
    let access = authorized.into_access();
    let name = validate::name(&request.name)?;
    validate::url_slug(&request.url_slug)?;
    let switcher_allocation = allocation(request.switcher_allocation)?;
    ensure_not_suspended(&state, access.organization_id).await?;
    ensure_client_room(&state, access.organization_id, 1).await?;

    let workspace = db::organizations::create_client(
        &state.db,
        access.organization_id,
        access.user.id(),
        name,
        &request.url_slug,
        switcher_allocation,
    )
    .await
    .map_err(|error| match error.constraint() {
        Some("workspaces_url_slug_key") => {
            ApiError::conflict("slug_taken", "This URL is already taken")
        }
        _ => error.into(),
    })?;
    record_team(
        &state,
        &access,
        "client_created",
        Some(&workspace.name),
        serde_json::json!({
            "url_slug": workspace.url_slug,
            "switcher_allocation": switcher_allocation,
        }),
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(client_dto(&state, access.organization_id, workspace.id).await?),
    ))
}

/// Fails with 403 `client_limit` when `adding` more clients would pass [`MAX_CLIENTS`].
pub(crate) async fn ensure_client_room(
    state: &ApiState,
    organization_id: Uuid,
    adding: usize,
) -> ApiResult<()> {
    let count = db::organizations::client_count(&state.db, organization_id).await?;
    if count + adding as i64 > MAX_CLIENTS {
        return Err(ApiError::forbidden(
            "client_limit",
            format!("An organization can have at most {MAX_CLIENTS} clients"),
        ));
    }
    Ok(())
}

pub(crate) async fn ensure_not_suspended(state: &ApiState, organization_id: Uuid) -> ApiResult<()> {
    let organization = db::organizations::get_by_id(&state.db, organization_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Organization not found"))?;
    if let Some(reason) = organization
        .suspended_reason
        .filter(|_| organization.suspended_at.is_some())
    {
        return Err(ApiError::forbidden(
            "organization_suspended",
            format!("NOALBS suspended this organization: {reason}"),
        ));
    }
    Ok(())
}

/// 409 `limit_below_usage`: a switcher limit can't go below the switchers already there,
/// since a limit never stops a running switcher.
pub(crate) fn limit_below_usage(what: &str, used: i64) -> ApiError {
    ApiError::conflict(
        "limit_below_usage",
        format!(
            "{what} can't be lower than the {used} switcher{} in use. Delete some first.",
            if used == 1 { "" } else { "s" }
        ),
    )
}

/// Rename a client or change its switcher allocation, which can't go below the
/// switchers it already has (409 `limit_below_usage`).
#[utoipa::path(put, path = "/organizations/{organization_id}/clients/{workspace_id}", tag = "organizations",
    params(("organization_id" = Uuid, Path), ("workspace_id" = Uuid, Path)),
    request_body = UpdateClient,
    responses(
        (status = OK, body = ClientDto),
        (status = NOT_FOUND, body = ErrorBody),
        (status = CONFLICT, description = "`limit_below_usage`", body = ErrorBody),
    ))]
async fn update_client(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgManage>,
    Path(path): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateClient>,
) -> ApiResult<Json<ClientDto>> {
    let (_, workspace_id) = path;
    let access = authorized.into_access();
    let name = validate::name(&request.name)?;
    let switcher_allocation = allocation(request.switcher_allocation)?;
    let before = db::organizations::get_client(&state.db, access.organization_id, workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Client not found"))?;

    match db::organizations::update_client(
        &state.db,
        access.organization_id,
        workspace_id,
        name,
        switcher_allocation,
    )
    .await?
    {
        db::organizations::LimitUpdate::Updated(_) => {}
        db::organizations::LimitUpdate::NotFound => {
            return Err(ApiError::not_found("Client not found"));
        }
        db::organizations::LimitUpdate::BelowUsage { used } => {
            return Err(limit_below_usage("The allocation", used));
        }
    }
    // A limit never stops a running switcher, so obs-link needs no nudge.
    record_team(
        &state,
        &access,
        "client_updated",
        Some(name),
        serde_json::json!({
            "previous_name": before.name,
            "switcher_allocation": switcher_allocation,
            "previous_allocation": before.switcher_allocation,
        }),
    )
    .await?;

    Ok(Json(
        client_dto(&state, access.organization_id, workspace_id).await?,
    ))
}

/// Delete a client workspace and everything in it (its switchers and their
/// history, its customers' membership).
#[utoipa::path(delete, path = "/organizations/{organization_id}/clients/{workspace_id}", tag = "organizations",
    params(("organization_id" = Uuid, Path), ("workspace_id" = Uuid, Path)),
    responses((status = NO_CONTENT, description = "Deleted"), (status = NOT_FOUND, body = ErrorBody)))]
async fn delete_client(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgManage>,
    Path(path): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let (_, workspace_id) = path;
    let access = authorized.into_access();
    let workspace = db::workspaces::get_by_id(&state.db, workspace_id)
        .await?
        .filter(|workspace| workspace.organization_id == Some(access.organization_id))
        .ok_or_else(|| ApiError::not_found("Client not found"))?;

    crate::privacy::delete_workspace(&state, workspace.id, None).await?;
    tracing::info!(organization_id = %access.organization_id, workspace_id = %workspace.id, by = %access.user.id(), "Deleted a client workspace");
    record_team(
        &state,
        &access,
        "client_deleted",
        Some(&workspace.name),
        serde_json::json!({ "url_slug": workspace.url_slug }),
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

// --- Team -------------------------------------------------------------------

/// A team member and the profile of their most recently used login.
#[derive(Debug, Serialize, ToSchema)]
pub struct TeamMemberDto {
    pub user_id: Uuid,
    pub role: OrganizationRole,
    pub username: Option<String>,
    pub avatar_url: Option<String>,
    /// The login the profile comes from, e.g. `TWITCH`.
    pub provider: Option<String>,
    pub joined_at: chrono::DateTime<chrono::Utc>,
}

impl From<db::organizations::TeamMember> for TeamMemberDto {
    fn from(member: db::organizations::TeamMember) -> Self {
        Self {
            user_id: member.user_id,
            role: member.role.into(),
            username: member.username,
            avatar_url: member.avatar_url,
            provider: member.provider,
            joined_at: member.created_at,
        }
    }
}

/// List an organization's team.
#[utoipa::path(get, path = "/organizations/{organization_id}/members", tag = "organizations",
    params(("organization_id" = Uuid, Path)),
    responses((status = OK, body = Vec<TeamMemberDto>), (status = NOT_FOUND, body = ErrorBody)))]
async fn list_team(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgView>,
) -> ApiResult<Json<Vec<TeamMemberDto>>> {
    let access = authorized.into_access();
    let members = db::organizations::list_members(&state.db, access.organization_id).await?;

    Ok(Json(members.into_iter().map(Into::into).collect()))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateTeamMember {
    pub role: OrganizationRole,
}

fn last_admin() -> ApiError {
    ApiError::conflict(
        "last_admin",
        "The organization needs at least one admin. Make someone else an admin first.",
    )
}

/// Change a team member's role.
#[utoipa::path(patch, path = "/organizations/{organization_id}/members/{user_id}", tag = "organizations",
    params(("organization_id" = Uuid, Path), ("user_id" = Uuid, Path)),
    request_body = UpdateTeamMember,
    responses(
        (status = NO_CONTENT, description = "Role changed"),
        (status = NOT_FOUND, body = ErrorBody),
        (status = CONFLICT, description = "`last_admin`", body = ErrorBody),
    ))]
async fn update_team_member(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgMembers>,
    Path(path): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateTeamMember>,
) -> ApiResult<StatusCode> {
    let (_, user_id) = path;
    let access = authorized.into_access();
    let previous = db::organizations::member_role(&state.db, access.organization_id, user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Member not found"))?;

    if !db::organizations::update_member_role(
        &state.db,
        access.organization_id,
        user_id,
        request.role.into(),
    )
    .await?
    {
        return Err(last_admin());
    }
    let name = user_display_name(&state, user_id).await?;
    record_team(
        &state,
        &access,
        "team_role_changed",
        Some(&name),
        serde_json::json!({
            "role": request.role,
            "previous_role": OrganizationRole::from(previous),
        }),
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Remove a team member, or leave the team when `user_id` is yourself. The last
/// admin can't go.
#[utoipa::path(delete, path = "/organizations/{organization_id}/members/{user_id}", tag = "organizations",
    params(("organization_id" = Uuid, Path), ("user_id" = Uuid, Path)),
    responses(
        (status = NO_CONTENT, description = "Removed"),
        (status = FORBIDDEN, body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
        (status = CONFLICT, description = "`last_admin`", body = ErrorBody),
    ))]
async fn remove_team_member(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgView>,
    Path(path): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let (_, user_id) = path;
    let access = authorized.into_access();

    // Anyone may leave; removing someone else is for admins.
    if user_id != access.user.id()
        && !db::organizations::has_permission(
            &state.db,
            access.organization_id,
            access.user.id(),
            db::organizations::OrganizationPermission::MEMBERS,
        )
        .await?
    {
        return Err(ApiError::forbidden(
            "forbidden",
            "You can't remove this member",
        ));
    }
    db::organizations::member_role(&state.db, access.organization_id, user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Member not found"))?;

    let name = user_display_name(&state, user_id).await?;
    if !db::organizations::remove_member(&state.db, access.organization_id, user_id).await? {
        return Err(last_admin());
    }
    let action = if user_id == access.user.id() {
        "team_member_left"
    } else {
        "team_member_removed"
    };
    record_team(&state, &access, action, Some(&name), serde_json::json!({})).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// A team invite that can still be used. The token is only shown once.
#[derive(Debug, Serialize, ToSchema)]
pub struct TeamInviteDto {
    pub id: Uuid,
    pub role: OrganizationRole,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<db::organizations::TeamInvite> for TeamInviteDto {
    fn from(invite: db::organizations::TeamInvite) -> Self {
        Self {
            id: invite.id,
            role: invite.role.into(),
            expires_at: invite.expires_at,
            created_at: invite.created_at,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateTeamInvite {
    pub role: OrganizationRole,
    /// 1 to 30 days; 7 when left out.
    pub expires_in_days: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreatedTeamInvite {
    pub invite: TeamInviteDto,
    /// The secret for the invite link (`/org-invite/{token}`). Not retrievable later.
    pub token: String,
}

/// Creates a team invite; `invited_by` is `None` for staff.
pub(crate) async fn create_invite(
    state: &ApiState,
    organization_id: Uuid,
    role: OrganizationRole,
    expires_in_days: Option<i64>,
    invited_by: Option<Uuid>,
) -> ApiResult<CreatedTeamInvite> {
    let days = expires_in_days.unwrap_or(DEFAULT_EXPIRY_DAYS);
    if !(1..=MAX_EXPIRY_DAYS).contains(&days) {
        return Err(ApiError::bad_request(
            "invalid_expiry",
            "Invites expire after 1 to 30 days",
        ));
    }

    let token = common::tokens::generate();
    let invite = db::organizations::create_invite(
        &state.db,
        organization_id,
        role.into(),
        invited_by,
        &common::tokens::hash(&token),
        Utc::now() + Duration::days(days),
    )
    .await?;

    Ok(CreatedTeamInvite {
        invite: invite.into(),
        token,
    })
}

/// List an organization's open team invites.
#[utoipa::path(get, path = "/organizations/{organization_id}/invites", tag = "organizations",
    params(("organization_id" = Uuid, Path)),
    responses((status = OK, body = Vec<TeamInviteDto>), (status = NOT_FOUND, body = ErrorBody)))]
async fn list_team_invites(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgMembers>,
) -> ApiResult<Json<Vec<TeamInviteDto>>> {
    let access = authorized.into_access();
    let invites = db::organizations::list_open_invites(&state.db, access.organization_id).await?;

    Ok(Json(invites.into_iter().map(Into::into).collect()))
}

/// Create an invite link for someone joining the team.
#[utoipa::path(post, path = "/organizations/{organization_id}/invites", tag = "organizations",
    params(("organization_id" = Uuid, Path)),
    request_body = CreateTeamInvite,
    responses((status = CREATED, body = CreatedTeamInvite), (status = BAD_REQUEST, body = ErrorBody)))]
async fn create_team_invite(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgMembers>,
    Json(request): Json<CreateTeamInvite>,
) -> ApiResult<(StatusCode, Json<CreatedTeamInvite>)> {
    let access = authorized.into_access();
    let created = create_invite(
        &state,
        access.organization_id,
        request.role,
        request.expires_in_days,
        Some(access.user.id()),
    )
    .await?;
    record_team(
        &state,
        &access,
        "team_invite_created",
        None,
        serde_json::json!({
            "role": request.role,
            "expires_at": created.invite.expires_at,
        }),
    )
    .await?;

    Ok((StatusCode::CREATED, Json(created)))
}

/// Revoke an open team invite.
#[utoipa::path(delete, path = "/organizations/{organization_id}/invites/{invite_id}", tag = "organizations",
    params(("organization_id" = Uuid, Path), ("invite_id" = Uuid, Path)),
    responses((status = NO_CONTENT, description = "Revoked"), (status = NOT_FOUND, body = ErrorBody)))]
async fn revoke_team_invite(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgMembers>,
    Path(path): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let (_, invite_id) = path;
    let access = authorized.into_access();

    if db::organizations::revoke_invite(&state.db, access.organization_id, invite_id).await? {
        record_team(
            &state,
            &access,
            "team_invite_revoked",
            None,
            serde_json::json!({}),
        )
        .await?;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("Invite not found"))
    }
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamInviteStatus {
    Open,
    Expired,
    Revoked,
    Used,
}

/// What someone opening a team invite link sees.
#[derive(Debug, Serialize, ToSchema)]
pub struct TeamInvitePreviewDto {
    pub organization_name: String,
    pub organization_slug: String,
    pub role: OrganizationRole,
    pub invited_by: Option<String>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub status: TeamInviteStatus,
    /// Whether the current user is on the team already.
    pub already_member: bool,
}

/// Look up a team invite link.
#[utoipa::path(get, path = "/organization-invites/{token}", tag = "organizations",
    params(("token" = String, Path)),
    responses((status = OK, body = TeamInvitePreviewDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn preview_team_invite(
    State(state): State<ApiState>,
    current_user: CurrentUser,
    Path(token): Path<String>,
) -> ApiResult<Json<TeamInvitePreviewDto>> {
    let invite = db::organizations::preview_invite(
        &state.db,
        &common::tokens::hash(&token),
        current_user.id(),
    )
    .await?
    .ok_or_else(|| ApiError::not_found("This invite link doesn't exist"))?;

    let status = if invite.revoked_at.is_some() {
        TeamInviteStatus::Revoked
    } else if invite.accepted_at.is_some() {
        TeamInviteStatus::Used
    } else if invite.expires_at <= Utc::now() {
        TeamInviteStatus::Expired
    } else {
        TeamInviteStatus::Open
    };

    Ok(Json(TeamInvitePreviewDto {
        organization_name: invite.organization_name,
        organization_slug: invite.organization_slug,
        role: invite.role.into(),
        invited_by: invite.invited_by_username,
        expires_at: invite.expires_at,
        status,
        already_member: invite.already_member,
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct JoinedOrganization {
    pub url_slug: String,
}

/// Accept a team invite and join the organization's team.
#[utoipa::path(post, path = "/organization-invites/{token}/accept", tag = "organizations",
    params(("token" = String, Path)),
    responses(
        (status = OK, body = JoinedOrganization),
        (status = NOT_FOUND, body = ErrorBody),
        (status = GONE, description = "`invite_expired`, `invite_revoked` or `invite_used`", body = ErrorBody),
    ))]
async fn accept_team_invite(
    State(state): State<ApiState>,
    current_user: CurrentUser,
    Path(token): Path<String>,
) -> ApiResult<Json<JoinedOrganization>> {
    let outcome = db::organizations::accept_invite(
        &state.db,
        &common::tokens::hash(&token),
        current_user.id(),
    )
    .await?;

    let gone = |code, message| ApiError::new(StatusCode::GONE, code, message);
    match outcome {
        AcceptOutcome::Joined { url_slug } => {
            if let Some(organization) = db::organizations::get_by_slug(&state.db, &url_slug).await?
            {
                record(
                    &state,
                    organization.id,
                    current_user.id(),
                    db::organization_audit::ActorKind::Team,
                    "team_member_joined",
                    None,
                    serde_json::json!({}),
                )
                .await?;
            }
            Ok(Json(JoinedOrganization { url_slug }))
        }
        AcceptOutcome::AlreadyMember { url_slug } => Ok(Json(JoinedOrganization { url_slug })),
        AcceptOutcome::NotFound => Err(ApiError::not_found("This invite link doesn't exist")),
        AcceptOutcome::Expired => Err(gone("invite_expired", "This invite has expired")),
        AcceptOutcome::Revoked => Err(gone("invite_revoked", "This invite was revoked")),
        AcceptOutcome::AlreadyUsed => Err(gone("invite_used", "This invite has already been used")),
    }
}
