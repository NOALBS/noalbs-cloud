//! Staff create organizations and set their pooled limits (there is no Stripe
//! plan for an organization yet: staff comp them, like a workspace plan), suspend
//! them and make the first team invite. Every action goes through
//! `admin_actions::record`. Handlers are prefixed `admin_` like in `admin.rs`.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::Staff,
    routes::{
        ApiError, ApiResult, ErrorBody,
        admin_actions::{Action, reason, record},
        organizations::{
            ClientPage, ClientsQuery, CreatedTeamInvite, OrganizationDto, OrganizationRole,
            TeamMemberDto, client_page, create_invite, limit_below_usage, summary,
        },
    },
    validate,
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(admin_list_organizations, admin_create_organization))
        .routes(routes!(admin_list_organization_clients))
        .routes(routes!(
            admin_get_organization,
            admin_update_organization,
            admin_delete_organization
        ))
        .routes(routes!(
            admin_suspend_organization,
            admin_lift_organization_suspension
        ))
        .routes(routes!(admin_invite_to_organization))
}

const MAX_SWITCHERS: i32 = 100_000;
const MAX_STREAM_SERVERS: i32 = 1_000;
const MAX_MEMBERS: i32 = 1_000;
const MAX_HISTORY_DAYS: i32 = 3_650;

/// The limits staff set on an organization.
#[derive(Debug, Deserialize, ToSchema)]
pub struct OrganizationLimitsInput {
    /// The pool: switchers in all its client workspaces together.
    pub max_switchers: i32,
    /// Per switcher.
    pub max_stream_servers: i32,
    /// Per client workspace.
    pub max_members: i32,
    pub history_days: i32,
}

impl OrganizationLimitsInput {
    fn validated(&self) -> ApiResult<db::organizations::Limits> {
        let within = |value: i32, max: i32| (0..=max).contains(&value);
        if !within(self.max_switchers, MAX_SWITCHERS)
            || !within(self.max_stream_servers, MAX_STREAM_SERVERS)
            || !within(self.max_members, MAX_MEMBERS)
            || !within(self.history_days, MAX_HISTORY_DAYS)
        {
            return Err(ApiError::bad_request(
                "invalid_limits",
                "A limit is out of range",
            ));
        }

        Ok(db::organizations::Limits {
            max_switchers: self.max_switchers,
            max_stream_servers: self.max_stream_servers,
            max_workspace_members: self.max_members,
            history_days: self.history_days,
        })
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AdminCreateOrganization {
    pub name: String,
    pub url_slug: String,
    pub limits: OrganizationLimitsInput,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AdminUpdateOrganization {
    pub name: String,
    pub limits: OrganizationLimitsInput,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AdminOrganizationSuspend {
    /// Shown to the organization's team: 10–500 characters.
    pub reason: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AdminOrganizationInvite {
    pub role: OrganizationRole,
    /// 1 to 30 days; 7 when left out.
    pub expires_in_days: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminOrganizationDetailDto {
    pub organization: OrganizationDto,
    pub team: Vec<TeamMemberDto>,
}

/// How an organization is named in the audit log.
fn subject(name: &str) -> String {
    format!("{name} (organization)")
}

fn details(organization_id: Uuid, name: &str) -> serde_json::Value {
    serde_json::json!({ "organization_id": organization_id, "organization_name": name })
}

/// Tells every switcher of the organization's clients to re-check what is paused
/// (and the chat service which bot speaks).
async fn notify_clients(state: &ApiState, organization_id: Uuid) -> ApiResult<()> {
    for workspace_id in db::organizations::workspace_ids(&state.db, organization_id).await? {
        state.notify_workspace(workspace_id).await;
    }
    Ok(())
}

async fn detail(
    state: &ApiState,
    organization_id: Uuid,
    staff: Uuid,
) -> ApiResult<AdminOrganizationDetailDto> {
    let organization = summary(state, organization_id, staff).await?;
    let team = db::organizations::list_members(&state.db, organization_id).await?;

    Ok(AdminOrganizationDetailDto {
        organization,
        team: team.into_iter().map(Into::into).collect(),
    })
}

/// A page of an organization's client workspaces, searched and filtered like its own
/// team sees them (without their live state).
#[utoipa::path(get, path = "/admin/organizations/{organization_id}/clients", tag = "admin",
    params(("organization_id" = Uuid, Path), ClientsQuery),
    responses((status = OK, body = ClientPage), (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_list_organization_clients(
    State(state): State<ApiState>,
    _staff: Staff,
    Path(organization_id): Path<Uuid>,
    Query(query): Query<ClientsQuery>,
) -> ApiResult<Json<ClientPage>> {
    db::organizations::get_by_id(&state.db, organization_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Organization not found"))?;

    Ok(Json(
        client_page(&state, organization_id, &query, false).await?,
    ))
}

/// List every organization.
#[utoipa::path(get, path = "/admin/organizations", tag = "admin",
    responses((status = OK, body = Vec<OrganizationDto>), (status = FORBIDDEN, body = ErrorBody)))]
async fn admin_list_organizations(
    State(state): State<ApiState>,
    Staff(staff): Staff,
) -> ApiResult<Json<Vec<OrganizationDto>>> {
    let organizations = db::organizations::list_all(&state.db, staff.id()).await?;

    Ok(Json(organizations.into_iter().map(Into::into).collect()))
}

/// Create an organization. Add its first admin with an invite.
#[utoipa::path(post, path = "/admin/organizations", tag = "admin",
    request_body = AdminCreateOrganization,
    responses(
        (status = CREATED, body = AdminOrganizationDetailDto),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = CONFLICT, description = "`slug_taken`", body = ErrorBody),
    ))]
async fn admin_create_organization(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Json(request): Json<AdminCreateOrganization>,
) -> ApiResult<(StatusCode, Json<AdminOrganizationDetailDto>)> {
    let name = validate::name(&request.name)?;
    validate::url_slug(&request.url_slug)?;
    let limits = request.limits.validated()?;

    let organization =
        db::organizations::create(&state.db, name, &request.url_slug, limits, staff.id())
            .await
            .map_err(|error| match error.constraint() {
                Some("organizations_url_slug_key") => {
                    ApiError::conflict("slug_taken", "This URL is already taken")
                }
                _ => error.into(),
            })?;
    record(
        &state,
        &staff,
        Action {
            action: "create_organization",
            route: "/admin/organizations",
            method: "POST",
            subject: Some(&subject(&organization.name)),
            details: details(organization.id, &organization.name),
            ..Action::default()
        },
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(detail(&state, organization.id, staff.id()).await?),
    ))
}

/// An organization with its team and client workspaces.
#[utoipa::path(get, path = "/admin/organizations/{organization_id}", tag = "admin",
    params(("organization_id" = Uuid, Path)),
    responses((status = OK, body = AdminOrganizationDetailDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_get_organization(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(organization_id): Path<Uuid>,
) -> ApiResult<Json<AdminOrganizationDetailDto>> {
    Ok(Json(detail(&state, organization_id, staff.id()).await?))
}

/// Rename an organization and change its limits. The switcher pool can't go
/// below the switchers its clients already have (409 `limit_below_usage`): a limit
/// never stops a running switcher.
#[utoipa::path(put, path = "/admin/organizations/{organization_id}", tag = "admin",
    params(("organization_id" = Uuid, Path)),
    request_body = AdminUpdateOrganization,
    responses(
        (status = OK, body = AdminOrganizationDetailDto),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
        (status = CONFLICT, description = "`limit_below_usage`", body = ErrorBody),
    ))]
async fn admin_update_organization(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(organization_id): Path<Uuid>,
    Json(request): Json<AdminUpdateOrganization>,
) -> ApiResult<Json<AdminOrganizationDetailDto>> {
    let name = validate::name(&request.name)?;
    let limits = request.limits.validated()?;

    let organization =
        match db::organizations::update(&state.db, organization_id, name, limits).await? {
            db::organizations::LimitUpdate::Updated(organization) => organization,
            db::organizations::LimitUpdate::NotFound => {
                return Err(ApiError::not_found("Organization not found"));
            }
            db::organizations::LimitUpdate::BelowUsage { used } => {
                return Err(limit_below_usage("The pool", used));
            }
        };
    record(
        &state,
        &staff,
        Action {
            action: "update_organization",
            route: "/admin/organizations/{organization_id}",
            method: "PUT",
            subject: Some(&subject(&organization.name)),
            details: serde_json::json!({
                "organization_id": organization.id,
                "organization_name": organization.name,
                "max_switchers": limits.max_switchers,
                "max_stream_servers": limits.max_stream_servers,
                "max_members": limits.max_workspace_members,
                "history_days": limits.history_days,
            }),
            ..Action::default()
        },
    )
    .await?;
    notify_clients(&state, organization_id).await?;

    Ok(Json(detail(&state, organization_id, staff.id()).await?))
}

/// Delete an organization that has no client workspaces left.
#[utoipa::path(delete, path = "/admin/organizations/{organization_id}", tag = "admin",
    params(("organization_id" = Uuid, Path)),
    responses(
        (status = NO_CONTENT, description = "Deleted"),
        (status = NOT_FOUND, body = ErrorBody),
        (status = CONFLICT, description = "`has_clients`", body = ErrorBody),
    ))]
async fn admin_delete_organization(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(organization_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let organization = db::organizations::get_by_id(&state.db, organization_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Organization not found"))?;

    if !db::organizations::delete_if_empty(&state.db, organization_id).await? {
        return Err(ApiError::conflict(
            "has_clients",
            "Delete or hand over the organization's client workspaces first",
        ));
    }
    record(
        &state,
        &staff,
        Action {
            action: "delete_organization",
            route: "/admin/organizations/{organization_id}",
            method: "DELETE",
            subject: Some(&subject(&organization.name)),
            details: details(organization.id, &organization.name),
            ..Action::default()
        },
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Suspend an organization: every client's switchers pause and their chat goes
/// quiet, and the team and customers see the reason. Again with another reason
/// updates it.
#[utoipa::path(put, path = "/admin/organizations/{organization_id}/suspension", tag = "admin",
    params(("organization_id" = Uuid, Path)),
    request_body = AdminOrganizationSuspend,
    responses(
        (status = OK, body = AdminOrganizationDetailDto),
        (status = BAD_REQUEST, description = "`invalid_reason`", body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ))]
async fn admin_suspend_organization(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(organization_id): Path<Uuid>,
    Json(request): Json<AdminOrganizationSuspend>,
) -> ApiResult<Json<AdminOrganizationDetailDto>> {
    let reason = reason(&request.reason)?;
    let organization = db::organizations::get_by_id(&state.db, organization_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Organization not found"))?;

    db::organizations::set_suspension(&state.db, organization_id, Some(reason)).await?;
    record(
        &state,
        &staff,
        Action {
            action: "suspend_organization",
            route: "/admin/organizations/{organization_id}/suspension",
            method: "PUT",
            reason: Some(reason),
            subject: Some(&subject(&organization.name)),
            details: details(organization.id, &organization.name),
            ..Action::default()
        },
    )
    .await?;
    notify_clients(&state, organization_id).await?;

    Ok(Json(detail(&state, organization_id, staff.id()).await?))
}

/// Lift an organization's suspension.
#[utoipa::path(delete, path = "/admin/organizations/{organization_id}/suspension", tag = "admin",
    params(("organization_id" = Uuid, Path)),
    responses((status = OK, body = AdminOrganizationDetailDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_lift_organization_suspension(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(organization_id): Path<Uuid>,
) -> ApiResult<Json<AdminOrganizationDetailDto>> {
    let organization = db::organizations::get_by_id(&state.db, organization_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Organization not found"))?;

    db::organizations::set_suspension(&state.db, organization_id, None).await?;
    record(
        &state,
        &staff,
        Action {
            action: "lift_organization_suspension",
            route: "/admin/organizations/{organization_id}/suspension",
            method: "DELETE",
            subject: Some(&subject(&organization.name)),
            details: details(organization.id, &organization.name),
            ..Action::default()
        },
    )
    .await?;
    notify_clients(&state, organization_id).await?;

    Ok(Json(detail(&state, organization_id, staff.id()).await?))
}

/// Invite someone to an organization's team, e.g. its first admin. The link is
/// `/org-invite/{token}`, shown once.
#[utoipa::path(post, path = "/admin/organizations/{organization_id}/invites", tag = "admin",
    params(("organization_id" = Uuid, Path)),
    request_body = AdminOrganizationInvite,
    responses(
        (status = CREATED, body = CreatedTeamInvite),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ))]
async fn admin_invite_to_organization(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(organization_id): Path<Uuid>,
    Json(request): Json<AdminOrganizationInvite>,
) -> ApiResult<(StatusCode, Json<CreatedTeamInvite>)> {
    let organization = db::organizations::get_by_id(&state.db, organization_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Organization not found"))?;

    let created = create_invite(
        &state,
        organization_id,
        request.role,
        request.expires_in_days,
        None,
    )
    .await?;
    record(
        &state,
        &staff,
        Action {
            action: "invite_to_organization",
            route: "/admin/organizations/{organization_id}/invites",
            method: "POST",
            subject: Some(&subject(&organization.name)),
            details: details(organization.id, &organization.name),
            ..Action::default()
        },
    )
    .await?;

    Ok((StatusCode::CREATED, Json(created)))
}
