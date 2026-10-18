//! Support access: a workspace lets NOALBS support in for a while, and reads
//! what staff did there (`permissions::Via::Support` logs it). Staff can open a
//! short, view-only break-glass grant without the workspace's permission, with
//! a reason the workspace sees.

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use chrono::{DateTime, TimeDelta, Utc};
use db::support::{SupportGrant, SupportLevel};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{Staff, SupportAccessManage, WorkspaceAuthorized},
    routes::{ApiError, ApiResult, ErrorBody, admin_actions},
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(
            get_support_access,
            grant_support_access,
            revoke_support_access
        ))
        .routes(routes!(support_access_log))
        .routes(routes!(acknowledge_break_glass))
        .routes(routes!(admin_break_glass))
}

/// The longest a grant can last: a week.
const MAX_HOURS: i64 = 7 * 24;
const DEFAULT_LOG_ENTRIES: i64 = 50;
const MAX_LOG_ENTRIES: i64 = 200;
/// Break-glass access is view-only and lasts an hour.
const BREAK_GLASS_HOURS: i64 = 1;
/// Acknowledged break-glass grants stay listed this long.
const BREAK_GLASS_LISTED_DAYS: i32 = 30;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupportLevelDto {
    /// Switchers, their settings, live data and history.
    View,
    /// Also switching scenes, starting/stopping and changing settings.
    Control,
}

impl From<SupportLevel> for SupportLevelDto {
    fn from(level: SupportLevel) -> Self {
        match level {
            SupportLevel::View => Self::View,
            SupportLevel::Control => Self::Control,
        }
    }
}

impl From<SupportLevelDto> for SupportLevel {
    fn from(level: SupportLevelDto) -> Self {
        match level {
            SupportLevelDto::View => Self::View,
            SupportLevelDto::Control => Self::Control,
        }
    }
}

/// The permission in force for NOALBS support.
#[derive(Debug, Serialize, ToSchema)]
pub struct SupportGrantDto {
    pub level: SupportLevelDto,
    /// Who allowed it; `null` once their account is gone, and for break-glass.
    pub granted_by: Option<String>,
    /// Opened by staff without the workspace's permission (`reason` says why).
    pub break_glass: bool,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl From<SupportGrant> for SupportGrantDto {
    fn from(grant: SupportGrant) -> Self {
        Self {
            level: grant.level.into(),
            granted_by: grant.granted_by_name,
            break_glass: grant.break_glass,
            reason: grant.reason,
            created_at: grant.created_at,
            expires_at: grant.expires_at,
        }
    }
}

/// Staff opening the workspace without its permission.
#[derive(Debug, Serialize, ToSchema)]
pub struct EmergencyAccessDto {
    pub id: Uuid,
    pub level: SupportLevelDto,
    /// The staff member; `null` once their account is gone.
    pub staff_name: Option<String>,
    pub reason: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// An owner or admin has marked it as seen.
    pub acknowledged: bool,
}

impl From<SupportGrant> for EmergencyAccessDto {
    fn from(grant: SupportGrant) -> Self {
        Self {
            id: grant.id,
            level: grant.level.into(),
            staff_name: grant.requested_by_name,
            reason: grant.reason.unwrap_or_default(),
            created_at: grant.created_at,
            expires_at: grant.expires_at,
            acknowledged: grant.acknowledged_at.is_some(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SupportAccessDto {
    /// The workspace's own permission; `null` when support has none now.
    pub grant: Option<SupportGrantDto>,
    /// Emergency access: not yet acknowledged, or from the last 30 days.
    pub emergency: Vec<EmergencyAccessDto>,
}

impl SupportAccessDto {
    async fn load(state: &ApiState, workspace_id: Uuid) -> ApiResult<Self> {
        let grant = db::support::active_grant(&state.db, workspace_id).await?;
        let emergency =
            db::support::break_glass_grants(&state.db, workspace_id, BREAK_GLASS_LISTED_DAYS)
                .await?;
        Ok(Self {
            grant: grant.map(Into::into),
            emergency: emergency.into_iter().map(Into::into).collect(),
        })
    }
}

/// What support may do in the workspace now, for staff: the workspace's grant,
/// else a break-glass one; `None` = nothing.
pub async fn grant_in_force(
    state: &ApiState,
    workspace_id: Uuid,
) -> ApiResult<Option<SupportGrantDto>> {
    Ok(db::support::active_grants(&state.db, workspace_id)
        .await?
        .into_iter()
        .next()
        .map(Into::into))
}

/// Whether NOALBS support may come into the workspace now.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/support-access", tag = "support",
    params(("workspace_id" = Uuid, Path)),
    responses((status = OK, body = SupportAccessDto), (status = FORBIDDEN, body = ErrorBody)))]
async fn get_support_access(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<SupportAccessManage>,
) -> ApiResult<Json<SupportAccessDto>> {
    let workspace_id = authorized.into_access().workspace_id;
    Ok(Json(SupportAccessDto::load(&state, workspace_id).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct GrantSupportAccess {
    pub level: SupportLevelDto,
    /// How long, 1–168 hours.
    pub hours: i64,
}

/// Let NOALBS support in for a while, replacing the permission in force.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/support-access", tag = "support",
    params(("workspace_id" = Uuid, Path)),
    request_body = GrantSupportAccess,
    responses(
        (status = OK, body = SupportAccessDto),
        (status = BAD_REQUEST, description = "`invalid_duration`", body = ErrorBody),
        (status = FORBIDDEN, description = "`members_only` for NOALBS support", body = ErrorBody),
    ))]
async fn grant_support_access(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<SupportAccessManage>,
    Json(request): Json<GrantSupportAccess>,
) -> ApiResult<Json<SupportAccessDto>> {
    let access = authorized.into_access();
    access.members_only()?;
    if !(1..=MAX_HOURS).contains(&request.hours) {
        return Err(ApiError::bad_request(
            "invalid_duration",
            format!("Support access lasts 1 to {MAX_HOURS} hours"),
        ));
    }

    db::support::grant(
        &state.db,
        access.workspace_id,
        request.level.into(),
        access.user.id(),
        Utc::now() + TimeDelta::hours(request.hours),
    )
    .await?;

    Ok(Json(
        SupportAccessDto::load(&state, access.workspace_id).await?,
    ))
}

/// End NOALBS support's permission now (not break-glass access, which lasts an hour).
#[utoipa::path(delete, path = "/workspaces/{workspace_id}/support-access", tag = "support",
    params(("workspace_id" = Uuid, Path)),
    responses(
        (status = NO_CONTENT, description = "Ended, or there was none"),
        (status = FORBIDDEN, description = "`members_only` for NOALBS support", body = ErrorBody),
    ))]
async fn revoke_support_access(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<SupportAccessManage>,
) -> ApiResult<StatusCode> {
    let access = authorized.into_access();
    access.members_only()?;
    db::support::revoke(&state.db, access.workspace_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Something NOALBS staff did in the workspace.
#[derive(Debug, Serialize, ToSchema)]
pub struct SupportAccessLogEntryDto {
    pub id: Uuid,
    pub staff_name: String,
    /// The HTTP method: `GET` is looking, anything else changing something.
    pub method: String,
    /// The API route, e.g. `/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scene`.
    pub route: String,
    pub switcher_id: Option<Uuid>,
    /// `null` when the switcher has been deleted since.
    pub switcher_name: Option<String>,
    /// The permission it was done under; `null` = without any (before support
    /// needed one).
    pub grant_level: Option<SupportLevelDto>,
    /// Done under break-glass access rather than the workspace's permission.
    pub break_glass: bool,
    /// The workspace's permission in force then, also when it didn't cover
    /// this (before support needed one); `null` = none was given.
    pub grant_in_force: Option<SupportLevelDto>,
    pub created_at: DateTime<Utc>,
}

impl From<db::support::AccessLogEntry> for SupportAccessLogEntryDto {
    fn from(entry: db::support::AccessLogEntry) -> Self {
        Self {
            id: entry.id,
            staff_name: entry.staff_name,
            method: entry.method,
            route: entry.route,
            switcher_id: entry.switcher_id,
            switcher_name: entry.switcher_name,
            grant_level: entry.grant_level.map(Into::into),
            break_glass: entry.break_glass,
            grant_in_force: entry.grant_in_force.map(Into::into),
            created_at: entry.created_at,
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct LogQuery {
    /// At most this many entries (default 50, up to 200).
    pub limit: Option<i64>,
    /// Only entries before this time, for paging back.
    pub before: Option<DateTime<Utc>>,
}

/// What NOALBS staff did in the workspace, newest first. Looking around is
/// listed once an hour per page; every change is listed.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/support-access/log", tag = "support",
    params(("workspace_id" = Uuid, Path), LogQuery),
    responses((status = OK, body = Vec<SupportAccessLogEntryDto>), (status = FORBIDDEN, body = ErrorBody)))]
async fn support_access_log(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<SupportAccessManage>,
    Query(query): Query<LogQuery>,
) -> ApiResult<Json<Vec<SupportAccessLogEntryDto>>> {
    let workspace_id = authorized.into_access().workspace_id;
    let limit = query
        .limit
        .unwrap_or(DEFAULT_LOG_ENTRIES)
        .clamp(1, MAX_LOG_ENTRIES);
    let entries = db::support::log(&state.db, workspace_id, query.before, limit).await?;

    Ok(Json(entries.into_iter().map(Into::into).collect()))
}

/// Mark every emergency access as seen, which hides its banner.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/support-access/acknowledge", tag = "support",
    params(("workspace_id" = Uuid, Path)),
    responses((status = OK, body = SupportAccessDto), (status = FORBIDDEN, body = ErrorBody)))]
async fn acknowledge_break_glass(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<SupportAccessManage>,
) -> ApiResult<Json<SupportAccessDto>> {
    let access = authorized.into_access();
    access.members_only()?;
    db::support::acknowledge_break_glass(&state.db, access.workspace_id).await?;

    Ok(Json(
        SupportAccessDto::load(&state, access.workspace_id).await?,
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BreakGlass {
    /// Why, for the workspace's owners and admins: 10–500 characters.
    pub reason: String,
    /// Only look, or also act on switchers (never members, billing or secrets).
    pub level: SupportLevelDto,
}

/// Staff: open a workspace without its permission, for abuse, a legal
/// obligation or a security problem. For an hour, to look or to act on switchers
/// (`level`); the workspace's owners and admins see who, what and why until
/// they acknowledge it.
#[utoipa::path(post, path = "/admin/workspaces/{workspace_id}/break-glass", tag = "admin",
    params(("workspace_id" = Uuid, Path)),
    request_body = BreakGlass,
    responses(
        (status = OK, body = SupportGrantDto),
        (status = BAD_REQUEST, description = "`invalid_reason`", body = ErrorBody),
        (status = FORBIDDEN, body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ))]
async fn admin_break_glass(
    State(state): State<ApiState>,
    Staff(user): Staff,
    axum::extract::Path(workspace_id): axum::extract::Path<Uuid>,
    Json(request): Json<BreakGlass>,
) -> ApiResult<Json<SupportGrantDto>> {
    let reason = admin_actions::reason(&request.reason)?;
    let workspace = db::workspaces::get_by_id(&state.db, workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Workspace not found"))?;

    let grant = db::support::break_glass(
        &state.db,
        workspace_id,
        user.id(),
        request.level.into(),
        reason,
        Utc::now() + TimeDelta::hours(BREAK_GLASS_HOURS),
    )
    .await?;
    admin_actions::record(
        &state,
        &user,
        admin_actions::Action {
            action: "break_glass",
            route: "/admin/workspaces/{workspace_id}/break-glass",
            method: "POST",
            workspace: Some((workspace_id, &workspace.name)),
            reason: Some(reason),
            details: serde_json::json!({ "level": request.level }),
            ..admin_actions::Action::default()
        },
    )
    .await?;

    Ok(Json(grant.into()))
}
