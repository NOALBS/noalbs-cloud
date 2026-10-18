//! Staff powers above workspaces (suspend a workspace, sign a user out
//! everywhere, disable a user, unlink a chat channel everywhere, reconnect a
//! switcher) and the staff audit log. Every action is recorded in
//! `staff_actions`, and in the affected workspace's support log (so its owners
//! and admins see it too). Handlers are prefixed `admin_` like in `admin.rs`.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{CurrentUser, Staff, user_display_name},
    routes::{
        ApiError, ApiResult, ErrorBody,
        admin::{AdminUserDetailDto, AdminWorkspaceDetailDto, user_detail, workspace_detail},
        support::SupportLevelDto,
    },
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(admin_suspend_workspace, admin_lift_suspension))
        .routes(routes!(admin_sign_out_everywhere))
        .routes(routes!(admin_disable_user, admin_enable_user))
        .routes(routes!(admin_unlink_chat_channel))
        .routes(routes!(admin_reconnect_switcher))
        .routes(routes!(admin_audit))
}

const MIN_REASON_LEN: usize = 10;
const MAX_REASON_LEN: usize = 500;
const DEFAULT_AUDIT_ENTRIES: i64 = 50;
const MAX_AUDIT_ENTRIES: i64 = 200;

/// A reason staff give for an action: 10–500 characters (`invalid_reason`).
pub(crate) fn reason(text: &str) -> ApiResult<&str> {
    let text = text.trim();
    if !(MIN_REASON_LEN..=MAX_REASON_LEN).contains(&text.chars().count()) {
        return Err(ApiError::bad_request(
            "invalid_reason",
            format!("Say why in {MIN_REASON_LEN} to {MAX_REASON_LEN} characters"),
        ));
    }
    Ok(text)
}

/// A staff action to record.
#[derive(Default)]
pub(crate) struct Action<'a> {
    /// Stable snake_case code, e.g. `suspend_workspace`.
    pub action: &'a str,
    /// The admin route and method, for the workspace's support log.
    pub route: &'a str,
    pub method: &'a str,
    pub workspace: Option<(Uuid, &'a str)>,
    /// What else it was about, named as it was then (e.g. an organization), for
    /// actions that aren't about a workspace. Shown as the target in the audit log.
    pub subject: Option<&'a str>,
    pub switcher: Option<(Uuid, &'a str)>,
    pub target_user: Option<Uuid>,
    pub reason: Option<&'a str>,
    pub details: serde_json::Value,
}

/// Records a staff action in `staff_actions`, and in the workspace's support
/// log when it's about a workspace.
pub(crate) async fn record(
    state: &ApiState,
    staff: &CurrentUser,
    action: Action<'_>,
) -> ApiResult<()> {
    let staff_name = user_display_name(state, staff.id()).await?;
    db::staff_actions::record(
        &state.db,
        &db::staff_actions::NewAction {
            staff_user_id: staff.id(),
            staff_name: &staff_name,
            action: action.action,
            workspace_id: action.workspace.map(|(id, _)| id),
            workspace_name: action.workspace.map(|(_, name)| name).or(action.subject),
            switcher_id: action.switcher.map(|(id, _)| id),
            switcher_name: action.switcher.map(|(_, name)| name),
            target_user_id: action.target_user,
            reason: action.reason,
            details: action.details,
        },
    )
    .await?;
    if let Some((workspace_id, _)) = action.workspace {
        db::support::log_access(
            &state.db,
            &db::support::Access {
                workspace_id,
                grant_id: None,
                staff_user_id: staff.id(),
                staff_name: &staff_name,
                method: action.method,
                route: action.route,
                switcher_id: action.switcher.map(|(id, _)| id),
            },
        )
        .await?;
    }
    tracing::warn!(
        staff = %staff.id(),
        action = action.action,
        workspace = ?action.workspace.map(|(id, _)| id),
        user = ?action.target_user,
        "Staff action"
    );
    Ok(())
}

async fn workspace_name(state: &ApiState, workspace_id: Uuid) -> ApiResult<String> {
    db::workspaces::get_by_id(&state.db, workspace_id)
        .await?
        .map(|workspace| workspace.name)
        .ok_or_else(|| ApiError::not_found("Workspace not found"))
}

// --- Workspaces -------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct AdminSuspend {
    /// Shown to the workspace's members: 10–500 characters.
    pub reason: String,
}

/// Suspend a workspace: its switchers pause, its chat links go quiet, and its
/// members see the reason. Again with another reason updates it.
#[utoipa::path(put, path = "/admin/workspaces/{workspace_id}/suspension", tag = "admin",
    params(("workspace_id" = Uuid, Path)),
    request_body = AdminSuspend,
    responses(
        (status = OK, body = AdminWorkspaceDetailDto),
        (status = BAD_REQUEST, description = "`invalid_reason`", body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ))]
async fn admin_suspend_workspace(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<AdminSuspend>,
) -> ApiResult<Json<AdminWorkspaceDetailDto>> {
    let reason = reason(&request.reason)?;
    let name = workspace_name(&state, workspace_id).await?;
    db::workspaces::set_suspension(&state.db, workspace_id, Some(reason)).await?;
    record(
        &state,
        &staff,
        Action {
            action: "suspend_workspace",
            route: "/admin/workspaces/{workspace_id}/suspension",
            method: "PUT",
            workspace: Some((workspace_id, &name)),
            reason: Some(reason),
            ..Action::default()
        },
    )
    .await?;
    // Every switcher reloads (and pauses); the chat service drops its links.
    state.notify_workspace(workspace_id).await;
    state.notify_chat().await;

    Ok(Json(workspace_detail(&state, workspace_id).await?))
}

/// Lift a workspace's suspension.
#[utoipa::path(delete, path = "/admin/workspaces/{workspace_id}/suspension", tag = "admin",
    params(("workspace_id" = Uuid, Path)),
    responses((status = OK, body = AdminWorkspaceDetailDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_lift_suspension(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(workspace_id): Path<Uuid>,
) -> ApiResult<Json<AdminWorkspaceDetailDto>> {
    let name = workspace_name(&state, workspace_id).await?;
    db::workspaces::set_suspension(&state.db, workspace_id, None).await?;
    record(
        &state,
        &staff,
        Action {
            action: "lift_suspension",
            route: "/admin/workspaces/{workspace_id}/suspension",
            method: "DELETE",
            workspace: Some((workspace_id, &name)),
            ..Action::default()
        },
    )
    .await?;
    state.notify_workspace(workspace_id).await;
    state.notify_chat().await;

    Ok(Json(workspace_detail(&state, workspace_id).await?))
}

// --- Users ------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminSignedOut {
    /// How many sessions ended.
    pub sessions: u64,
}

fn not_yourself(staff: &CurrentUser, user_id: Uuid) -> ApiResult<()> {
    if staff.id() == user_id {
        return Err(ApiError::conflict(
            "own_account",
            "Use your own account page for your own sessions.",
        ));
    }
    Ok(())
}

/// Sign a user out everywhere, e.g. after their account was compromised.
#[utoipa::path(delete, path = "/admin/users/{user_id}/sessions", tag = "admin",
    params(("user_id" = Uuid, Path)),
    responses(
        (status = OK, body = AdminSignedOut),
        (status = CONFLICT, description = "`own_account`", body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ))]
async fn admin_sign_out_everywhere(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(user_id): Path<Uuid>,
) -> ApiResult<Json<AdminSignedOut>> {
    not_yourself(&staff, user_id)?;
    if db::users::get_by_id(&state.db, user_id).await?.is_none() {
        return Err(ApiError::not_found("User not found"));
    }
    let sessions = db::sessions::delete_all_for_user(&state.db, user_id).await?;
    record(
        &state,
        &staff,
        Action {
            action: "sign_out_everywhere",
            target_user: Some(user_id),
            details: serde_json::json!({ "sessions": sessions }),
            ..Action::default()
        },
    )
    .await?;

    Ok(Json(AdminSignedOut { sessions }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AdminDisable {
    /// Why, for staff (and the user, when they ask): 10–500 characters.
    pub reason: String,
}

/// Disable a user: they're signed out and can't sign in. Not for staff (remove
/// their staff access first) or yourself.
#[utoipa::path(put, path = "/admin/users/{user_id}/disabled", tag = "admin",
    params(("user_id" = Uuid, Path)),
    request_body = AdminDisable,
    responses(
        (status = OK, body = AdminUserDetailDto),
        (status = BAD_REQUEST, description = "`invalid_reason`", body = ErrorBody),
        (status = CONFLICT, description = "`own_account`, `staff_account`", body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ))]
async fn admin_disable_user(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(user_id): Path<Uuid>,
    Json(request): Json<AdminDisable>,
) -> ApiResult<Json<AdminUserDetailDto>> {
    not_yourself(&staff, user_id)?;
    let reason = reason(&request.reason)?;
    let user = db::users::get_by_id(&state.db, user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("User not found"))?;
    if user.staff {
        return Err(ApiError::conflict(
            "staff_account",
            "Remove their staff access before disabling the account.",
        ));
    }
    db::users::set_disabled(&state.db, user_id, Some(reason)).await?;
    record(
        &state,
        &staff,
        Action {
            action: "disable_user",
            target_user: Some(user_id),
            reason: Some(reason),
            ..Action::default()
        },
    )
    .await?;

    Ok(Json(user_detail(&state, user_id).await?))
}

/// Let a disabled user sign in again.
#[utoipa::path(delete, path = "/admin/users/{user_id}/disabled", tag = "admin",
    params(("user_id" = Uuid, Path)),
    responses((status = OK, body = AdminUserDetailDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_enable_user(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(user_id): Path<Uuid>,
) -> ApiResult<Json<AdminUserDetailDto>> {
    if !db::users::set_disabled(&state.db, user_id, None).await? {
        return Err(ApiError::not_found("User not found"));
    }
    record(
        &state,
        &staff,
        Action {
            action: "enable_user",
            target_user: Some(user_id),
            ..Action::default()
        },
    )
    .await?;

    Ok(Json(user_detail(&state, user_id).await?))
}

// --- Chat and switchers -----------------------------------------------------

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminUnlinked {
    /// How many switchers it was linked to.
    pub links: usize,
}

/// Unlink a chat channel from every switcher, in every workspace: the bot
/// leaves it. Its owner can link it again.
#[utoipa::path(delete, path = "/admin/chat-channels/{channel_id}/links", tag = "admin",
    params(("channel_id" = Uuid, Path)),
    responses((status = OK, body = AdminUnlinked), (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_unlink_chat_channel(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(channel_id): Path<Uuid>,
) -> ApiResult<Json<AdminUnlinked>> {
    let channel = db::chat::channel(&state.db, channel_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Chat channel not found"))?;
    let links = db::chat::unlink_everywhere(&state.db, channel_id).await?;
    let details = serde_json::json!({
        "platform": channel.platform.as_str(),
        "channel": channel.login,
    });
    for link in &links {
        record(
            &state,
            &staff,
            Action {
                action: "unlink_chat_channel",
                route: "/admin/chat-channels/{channel_id}/links",
                method: "DELETE",
                workspace: Some((link.workspace_id, &link.workspace_name)),
                switcher: Some((link.switcher_id, &link.switcher_name)),
                details: details.clone(),
                ..Action::default()
            },
        )
        .await?;
    }
    state.notify_chat().await;

    Ok(Json(AdminUnlinked { links: links.len() }))
}

/// Close a switcher's browser-source connection so it reconnects, e.g. when
/// it's stuck. Nothing changes for the streamer beyond a second's reconnect.
#[utoipa::path(post, path = "/admin/switchers/{switcher_id}/reconnect", tag = "admin",
    params(("switcher_id" = Uuid, Path)),
    responses((status = NO_CONTENT, description = "Asked"), (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_reconnect_switcher(
    State(state): State<ApiState>,
    Staff(staff): Staff,
    Path(switcher_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let switcher = db::switchers::get(&state.db, switcher_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Switcher not found"))?;
    let name = workspace_name(&state, switcher.workspace_id).await?;
    record(
        &state,
        &staff,
        Action {
            action: "reconnect_switcher",
            route: "/admin/switchers/{switcher_id}/reconnect",
            method: "POST",
            workspace: Some((switcher.workspace_id, &name)),
            switcher: Some((switcher.id, &switcher.name)),
            ..Action::default()
        },
    )
    .await?;
    if let Err(error) = state
        .nats
        .publish(live::reconnect_subject(switcher_id), "".into())
        .await
    {
        tracing::error!(%error, %switcher_id, "Couldn't ask obs-link to reconnect");
        return Err(ApiError::internal());
    }

    Ok(StatusCode::NO_CONTENT)
}

// --- Audit log --------------------------------------------------------------

/// Something staff did: from Admin (`kind` `admin`, `action` a code like
/// `suspend_workspace`) or inside a workspace as support (`kind` `support`,
/// `action` the API route, with `method`).
#[derive(Debug, Serialize, ToSchema)]
pub struct AdminAuditEntryDto {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub staff_name: String,
    pub kind: String,
    pub action: String,
    pub method: Option<String>,
    pub workspace_id: Option<Uuid>,
    pub workspace_name: Option<String>,
    /// `null` once the workspace is deleted.
    pub workspace_slug: Option<String>,
    pub switcher_name: Option<String>,
    pub target_user_id: Option<Uuid>,
    pub target_user_name: Option<String>,
    pub reason: Option<String>,
    /// Support: the permission it was done under, and whether break-glass.
    pub grant_level: Option<SupportLevelDto>,
    pub break_glass: bool,
}

impl From<db::staff_actions::AuditEntry> for AdminAuditEntryDto {
    fn from(entry: db::staff_actions::AuditEntry) -> Self {
        Self {
            id: entry.id,
            created_at: entry.created_at,
            staff_name: entry.staff_name,
            kind: entry.kind,
            action: entry.action,
            method: entry.method,
            workspace_id: entry.workspace_id,
            workspace_name: entry.workspace_name,
            workspace_slug: entry.workspace_slug,
            switcher_name: entry.switcher_name,
            target_user_id: entry.target_user_id,
            target_user_name: entry.target_user_name,
            reason: entry.reason,
            grant_level: entry.grant_level.map(Into::into),
            break_glass: entry.break_glass,
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct AuditQuery {
    /// `admin` or `support`; both when left out.
    pub kind: Option<String>,
    /// At most this many entries (default 50, up to 200).
    pub limit: Option<i64>,
    /// Only entries before this time, for paging back.
    pub before: Option<DateTime<Utc>>,
}

/// What staff did, newest first: Admin actions and support work in workspaces.
#[utoipa::path(get, path = "/admin/audit", tag = "admin", params(AuditQuery),
    responses((status = OK, body = Vec<AdminAuditEntryDto>), (status = BAD_REQUEST, body = ErrorBody)))]
async fn admin_audit(
    State(state): State<ApiState>,
    Staff(_staff): Staff,
    Query(query): Query<AuditQuery>,
) -> ApiResult<Json<Vec<AdminAuditEntryDto>>> {
    let kind = match query.kind.as_deref() {
        None | Some("") => None,
        Some(kind @ ("admin" | "support")) => Some(kind),
        Some(_) => {
            return Err(ApiError::bad_request(
                "invalid_kind",
                "kind is admin or support",
            ));
        }
    };
    let limit = query
        .limit
        .unwrap_or(DEFAULT_AUDIT_ENTRIES)
        .clamp(1, MAX_AUDIT_ENTRIES);
    let entries = db::staff_actions::audit(&state.db, kind, query.before, limit).await?;

    Ok(Json(entries.into_iter().map(Into::into).collect()))
}
