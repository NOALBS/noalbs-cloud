use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::{Duration, Utc};
use db::{invites::AcceptOutcome, workspaces::WorkspaceRoleEnum};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{AccountManage, CurrentUser, WorkspaceAuthorized},
    routes::{
        ApiError, ApiResult, ErrorBody,
        organization_audit::{record, record_in_client},
        workspaces::WorkspaceRole,
    },
};

const DEFAULT_EXPIRY_DAYS: i64 = 7;
const MAX_EXPIRY_DAYS: i64 = 30;

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_invites, create_invite))
        .routes(routes!(revoke_invite))
        .routes(routes!(preview_invite))
        .routes(routes!(accept_invite))
}

/// An invite that can still be used. The token is only shown once, when the
/// invite is created.
#[derive(Debug, Serialize, ToSchema)]
pub struct InviteDto {
    pub id: Uuid,
    pub role: WorkspaceRole,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<db::invites::Invite> for InviteDto {
    fn from(invite: db::invites::Invite) -> Self {
        Self {
            id: invite.id,
            role: invite.role.into(),
            expires_at: invite.expires_at,
            created_at: invite.created_at,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateInvite {
    /// `operator` or `admin`.
    pub role: WorkspaceRole,
    /// 1 to 30 days; 7 when left out.
    pub expires_in_days: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreatedInvite {
    pub invite: InviteDto,
    /// The secret for the invite link (`/invite/{token}`). Not retrievable later.
    pub token: String,
}

/// List a workspace's open invites.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/invites", tag = "invites",
    params(("workspace_id" = Uuid, Path)),
    responses((status = OK, body = Vec<InviteDto>), (status = FORBIDDEN, body = ErrorBody)))]
async fn list_invites(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<AccountManage>,
) -> ApiResult<Json<Vec<InviteDto>>> {
    let access = authorized.into_access();
    let invites = db::invites::list_open(&state.db, access.workspace_id).await?;

    Ok(Json(invites.into_iter().map(Into::into).collect()))
}

/// Create an invite link. Only the owner can invite admins.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/invites", tag = "invites",
    params(("workspace_id" = Uuid, Path)),
    request_body = CreateInvite,
    responses(
        (status = CREATED, body = CreatedInvite),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = FORBIDDEN, description = "`plan_limit` when the workspace is full", body = ErrorBody),
    ))]
async fn create_invite(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<AccountManage>,
    Json(request): Json<CreateInvite>,
) -> ApiResult<(StatusCode, Json<CreatedInvite>)> {
    let access = authorized.into_access();
    let role: WorkspaceRoleEnum = request.role.into();

    if role == WorkspaceRoleEnum::Owner {
        return Err(ApiError::bad_request(
            "invalid_role",
            "A workspace has exactly one owner",
        ));
    }
    if role == WorkspaceRoleEnum::Admin && !access.actor(&state).await?.is_owner_or_staff() {
        return Err(ApiError::forbidden(
            "forbidden",
            "Only the owner can invite admins",
        ));
    }

    let days = request.expires_in_days.unwrap_or(DEFAULT_EXPIRY_DAYS);
    if !(1..=MAX_EXPIRY_DAYS).contains(&days) {
        return Err(ApiError::bad_request(
            "invalid_expiry",
            "Invites expire after 1 to 30 days",
        ));
    }

    super::plans::ensure_member_room(&state, access.workspace_id).await?;

    let token = common::tokens::generate();
    let invite = db::invites::create(
        &state.db,
        access.workspace_id,
        role,
        access.user.id(),
        &common::tokens::hash(&token),
        Utc::now() + Duration::days(days),
    )
    .await?;
    record_in_client(
        &state,
        &access,
        "customer_invite_created",
        None,
        serde_json::json!({ "role": request.role, "expires_at": invite.expires_at }),
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(CreatedInvite {
            invite: invite.into(),
            token,
        }),
    ))
}

/// Revoke an open invite.
#[utoipa::path(delete, path = "/workspaces/{workspace_id}/invites/{invite_id}", tag = "invites",
    params(("workspace_id" = Uuid, Path), ("invite_id" = Uuid, Path)),
    responses((status = NO_CONTENT, description = "Revoked"), (status = NOT_FOUND, body = ErrorBody)))]
async fn revoke_invite(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<AccountManage>,
    Path(path): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let (_, invite_id) = path;
    let access = authorized.into_access();

    if db::invites::revoke(&state.db, access.workspace_id, invite_id).await? {
        record_in_client(
            &state,
            &access,
            "customer_invite_revoked",
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
pub enum InviteStatus {
    Open,
    Expired,
    Revoked,
    Used,
}

/// What someone opening an invite link sees.
#[derive(Debug, Serialize, ToSchema)]
pub struct InvitePreviewDto {
    pub workspace_name: String,
    pub workspace_slug: String,
    /// The organization that manages and pays for this client workspace, if it is
    /// one: its team can see everything in it, and the invite never gives billing.
    pub managed_by: Option<String>,
    pub role: WorkspaceRole,
    pub invited_by: Option<String>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub status: InviteStatus,
    /// Whether the current user is already in this workspace.
    pub already_member: bool,
}

/// Look up an invite link.
#[utoipa::path(get, path = "/invites/{token}", tag = "invites",
    params(("token" = String, Path)),
    responses((status = OK, body = InvitePreviewDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn preview_invite(
    State(state): State<ApiState>,
    current_user: CurrentUser,
    Path(token): Path<String>,
) -> ApiResult<Json<InvitePreviewDto>> {
    let invite = db::invites::preview_by_token_hash(&state.db, &common::tokens::hash(&token))
        .await?
        .ok_or_else(|| ApiError::not_found("This invite link doesn't exist"))?;

    let status = if invite.revoked_at.is_some() {
        InviteStatus::Revoked
    } else if invite.accepted_at.is_some() {
        InviteStatus::Used
    } else if invite.expires_at <= Utc::now() {
        InviteStatus::Expired
    } else {
        InviteStatus::Open
    };
    let already_member =
        db::workspaces::member_role(&state.db, invite.workspace_id, current_user.id())
            .await?
            .is_some();

    Ok(Json(InvitePreviewDto {
        workspace_name: invite.workspace_name,
        workspace_slug: invite.workspace_slug,
        managed_by: invite.organization_name,
        role: invite.role.into(),
        invited_by: invite.invited_by_username,
        expires_at: invite.expires_at,
        status,
        already_member,
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct JoinedWorkspace {
    pub workspace_id: Uuid,
    pub url_slug: String,
}

/// Accept an invite and join its workspace.
#[utoipa::path(post, path = "/invites/{token}/accept", tag = "invites",
    params(("token" = String, Path)),
    responses(
        (status = OK, body = JoinedWorkspace),
        (status = NOT_FOUND, body = ErrorBody),
        (status = GONE, description = "`invite_expired`, `invite_revoked` or `invite_used`", body = ErrorBody),
        (status = FORBIDDEN, description = "`plan_limit` when the workspace is full", body = ErrorBody),
    ))]
async fn accept_invite(
    State(state): State<ApiState>,
    current_user: CurrentUser,
    Path(token): Path<String>,
) -> ApiResult<Json<JoinedWorkspace>> {
    let outcome =
        db::invites::accept(&state.db, &common::tokens::hash(&token), current_user.id()).await?;

    let gone = |code, message| ApiError::new(StatusCode::GONE, code, message);
    let joined = matches!(outcome, AcceptOutcome::Joined { .. });
    let (workspace_id, url_slug) = match outcome {
        AcceptOutcome::Joined {
            workspace_id,
            url_slug,
        }
        | AcceptOutcome::AlreadyMember {
            workspace_id,
            url_slug,
        } => (workspace_id, url_slug),
        AcceptOutcome::NotFound => {
            return Err(ApiError::not_found("This invite link doesn't exist"));
        }
        AcceptOutcome::Expired => {
            return Err(gone("invite_expired", "This invite has expired"));
        }
        AcceptOutcome::Revoked => {
            return Err(gone("invite_revoked", "This invite was revoked"));
        }
        AcceptOutcome::AlreadyUsed => {
            return Err(gone("invite_used", "This invite has already been used"));
        }
        AcceptOutcome::MemberLimitReached => return Err(super::plans::member_limit_error()),
    };

    db::users::set_last_workspace(&state.db, current_user.id(), workspace_id).await?;
    if joined
        && let Some(workspace) = db::workspaces::get_by_id(&state.db, workspace_id).await?
        && let Some(organization_id) = workspace.organization_id
    {
        record(
            &state,
            organization_id,
            current_user.id(),
            db::organization_audit::ActorKind::Customer,
            "customer_joined",
            Some(&workspace.name),
            serde_json::json!({}),
        )
        .await?;
    }

    Ok(Json(JoinedWorkspace {
        workspace_id,
        url_slug,
    }))
}
