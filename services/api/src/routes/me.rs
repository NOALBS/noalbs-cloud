use axum::{
    Json,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use axum_extra::extract::cookie::PrivateCookieJar;
use chrono::{DateTime, Utc};
use db::oauth2_connections::UnlinkOutcome;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{CurrentSession, CurrentUser, SESSION_COOKIE_NAME},
    routes::{ApiError, ApiResult, ErrorBody, auth::expired_cookie},
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(set_last_workspace))
        .routes(routes!(list_sessions))
        .routes(routes!(sign_out_session))
        .routes(routes!(sign_out_other_sessions))
        .routes(routes!(unlink_connection))
        .routes(routes!(export_data))
        .routes(routes!(delete_account))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LastWorkspace {
    pub workspace_id: Uuid,
}

/// Remember the workspace the user opened, so they return to it next time.
#[utoipa::path(put, path = "/me/last-workspace", tag = "auth",
    request_body = LastWorkspace,
    responses((status = NO_CONTENT, description = "Saved"), (status = NOT_FOUND, body = ErrorBody)))]
async fn set_last_workspace(
    State(state): State<ApiState>,
    current_user: CurrentUser,
    Json(request): Json<LastWorkspace>,
) -> ApiResult<StatusCode> {
    if db::users::set_last_workspace(&state.db, current_user.id(), request.workspace_id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found(
            "You are not a member of this workspace",
        ))
    }
}

/// A browser signed in to this account.
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionDto {
    pub id: Uuid,
    /// The session making this request.
    pub current: bool,
    /// The account it signed in with: `TWITCH`, `KICK` or `DEV`.
    pub provider: String,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Updated at most every few minutes.
    pub last_used_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// List where you're signed in, most recently used first.
#[utoipa::path(get, path = "/me/sessions", tag = "auth",
    responses((status = OK, body = Vec<SessionDto>), (status = UNAUTHORIZED, body = ErrorBody)))]
async fn list_sessions(
    State(state): State<ApiState>,
    session: CurrentSession,
) -> ApiResult<Json<Vec<SessionDto>>> {
    let sessions = db::sessions::list_for_user(&state.db, session.user_id).await?;
    Ok(Json(
        sessions
            .into_iter()
            .map(|item| SessionDto {
                current: item.id == session.session_id,
                id: item.id,
                provider: item.provider,
                user_agent: item.user_agent,
                created_at: item.created_at,
                last_used_at: item.last_used_at,
                expires_at: item.expires_at,
            })
            .collect(),
    ))
}

/// Sign out another browser. Use `POST /auth/logout` for this one.
#[utoipa::path(delete, path = "/me/sessions/{session_id}", tag = "auth",
    params(("session_id" = Uuid, Path)),
    responses(
        (status = NO_CONTENT, description = "Signed out"),
        (status = BAD_REQUEST, description = "`current_session`", body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ))]
async fn sign_out_session(
    State(state): State<ApiState>,
    session: CurrentSession,
    Path(session_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    if session_id == session.session_id {
        return Err(ApiError::bad_request(
            "current_session",
            "That's this browser. Use Sign out instead.",
        ));
    }
    if db::sessions::delete_for_user(&state.db, session.user_id, session_id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("Session not found"))
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SignedOut {
    pub signed_out: u64,
}

/// Sign out every browser except this one.
#[utoipa::path(post, path = "/me/sessions/sign-out-others", tag = "auth",
    responses((status = OK, body = SignedOut), (status = UNAUTHORIZED, body = ErrorBody)))]
async fn sign_out_other_sessions(
    State(state): State<ApiState>,
    session: CurrentSession,
) -> ApiResult<Json<SignedOut>> {
    let signed_out =
        db::sessions::delete_others(&state.db, session.user_id, session.session_id).await?;
    Ok(Json(SignedOut { signed_out }))
}

/// Disconnect a linked account. Browsers that signed in with it are signed out,
/// except this one. The last account can't be removed: you couldn't sign in.
#[utoipa::path(delete, path = "/me/connections/{connection_id}", tag = "auth",
    params(("connection_id" = Uuid, Path)),
    responses(
        (status = NO_CONTENT, description = "Disconnected"),
        (status = CONFLICT, description = "`last_connection`", body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ))]
async fn unlink_connection(
    State(state): State<ApiState>,
    session: CurrentSession,
    Path(connection_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    match db::oauth2_connections::unlink(
        &state.db,
        session.user_id,
        connection_id,
        session.session_id,
    )
    .await?
    {
        UnlinkOutcome::Unlinked => Ok(StatusCode::NO_CONTENT),
        UnlinkOutcome::NotFound => Err(ApiError::not_found("Connection not found")),
        UnlinkOutcome::LastConnection => Err(ApiError::conflict(
            "last_connection",
            "This is the only account you sign in with. Connect another one first.",
        )),
    }
}

/// Everything NOALBS Cloud keeps about you, for "Download my data" (GDPR access
/// and portability). Secrets (tokens, passwords, stream keys) are left out.
#[derive(Debug, Serialize, ToSchema)]
pub struct AccountExport {
    pub exported_at: DateTime<Utc>,
    pub account: ExportedAccount,
    /// The Twitch/Kick accounts you sign in with.
    pub connections: Vec<ExportedConnection>,
    /// Browsers signed in to your account.
    pub sessions: Vec<ExportedSession>,
    /// Workspaces you're in; switchers are listed for the ones you own.
    pub workspaces: Vec<ExportedWorkspace>,
    /// Organizations whose team you're on.
    pub organizations: Vec<ExportedOrganization>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ExportedOrganization {
    pub name: String,
    pub url_slug: String,
    pub role: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ExportedAccount {
    pub id: Uuid,
    pub staff: bool,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ExportedConnection {
    pub provider: String,
    pub username: String,
    pub avatar_url: Option<String>,
    pub connected_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ExportedSession {
    pub provider: String,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ExportedWorkspace {
    pub name: String,
    pub url_slug: String,
    pub role: Option<String>,
    /// The organization that manages this client workspace, if it is one.
    pub managed_by: Option<String>,
    pub switchers: Vec<ExportedSwitcher>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ExportedSwitcher {
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    /// Name and kind of each stream server (their URLs can hold keys, so they're left out).
    pub stream_servers: Vec<String>,
    /// Chat channels linked to it, as `platform/login`.
    pub chat_channels: Vec<String>,
}

/// Download everything NOALBS Cloud keeps about you, as JSON.
#[utoipa::path(get, path = "/me/export", tag = "auth",
    responses((status = OK, body = AccountExport)))]
async fn export_data(
    State(state): State<ApiState>,
    current_user: CurrentUser,
) -> ApiResult<impl IntoResponse> {
    let user_id = current_user.id();
    let user = db::users::get_by_id(&state.db, user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Account not found"))?;
    let connections = db::oauth2_connections::list_profiles_for_user(&state.db, user_id).await?;
    let sessions = db::sessions::list_for_user(&state.db, user_id).await?;

    let mut workspaces = Vec::new();
    for workspace in db::workspaces::list_summaries(&state.db, user_id).await? {
        let owner = matches!(
            workspace.role,
            Some(db::workspaces::WorkspaceRoleEnum::Owner)
        );
        let mut switchers = Vec::new();
        if owner {
            for switcher in db::switchers::list_summaries(&state.db, workspace.id).await? {
                let servers = db::stream_servers::by_switcher_id(&state.db, switcher.id).await?;
                let channels = db::chat::channels_for_switcher(&state.db, switcher.id).await?;
                switchers.push(ExportedSwitcher {
                    name: switcher.name,
                    created_at: switcher.created_at,
                    last_seen_at: switcher.last_seen_at,
                    stream_servers: servers
                        .into_iter()
                        .map(|server| format!("{} ({:?})", server.name, server.kind))
                        .collect(),
                    chat_channels: channels
                        .into_iter()
                        .map(|linked| {
                            format!(
                                "{}/{}",
                                format!("{:?}", linked.channel.platform).to_lowercase(),
                                linked.channel.login
                            )
                        })
                        .collect(),
                });
            }
        }
        workspaces.push(ExportedWorkspace {
            name: workspace.name,
            url_slug: workspace.url_slug,
            role: workspace
                .role
                .map(|role| format!("{role:?}").to_lowercase()),
            managed_by: workspace.organization_name,
            switchers,
        });
    }

    let export = AccountExport {
        exported_at: Utc::now(),
        account: ExportedAccount {
            id: user.id,
            staff: user.staff,
            created_at: user.created_at,
        },
        connections: connections
            .into_iter()
            .map(|connection| ExportedConnection {
                provider: connection.provider,
                username: connection.provider_username,
                avatar_url: connection.provider_avatar_url,
                connected_at: connection.created_at,
            })
            .collect(),
        sessions: sessions
            .into_iter()
            .map(|session| ExportedSession {
                provider: session.provider,
                user_agent: session.user_agent,
                created_at: session.created_at,
                last_used_at: session.last_used_at,
                expires_at: session.expires_at,
            })
            .collect(),
        workspaces,
        organizations: db::organizations::list_for_user(&state.db, user_id)
            .await?
            .into_iter()
            .map(|organization| ExportedOrganization {
                name: organization.name,
                url_slug: organization.url_slug,
                role: organization
                    .role
                    .map(|role| format!("{role:?}").to_lowercase()),
            })
            .collect(),
    };
    Ok((
        [(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"noalbs-cloud-data.json\"",
        )],
        Json(export),
    ))
}

/// Delete your account for good (GDPR erasure). Workspaces you own alone go with
/// it (a subscription ends at once, without a refund). If a workspace you own has
/// other members, transfer it, remove them or delete it first: 409 `owns_shared_workspace`.
#[utoipa::path(delete, path = "/me", tag = "auth",
    responses(
        (status = NO_CONTENT, description = "Deleted and signed out"),
        (status = CONFLICT, description = "`owns_shared_workspace` or `last_organization_admin`", body = ErrorBody),
        (status = BAD_GATEWAY, description = "`stripe_error`: nothing was deleted", body = ErrorBody),
    ))]
async fn delete_account(
    State(state): State<ApiState>,
    current_user: CurrentUser,
    cookies: PrivateCookieJar,
) -> ApiResult<impl IntoResponse> {
    let user_id = current_user.id();
    let owned = db::workspaces::owned_by(&state.db, user_id).await?;
    let shared: Vec<&str> = owned
        .iter()
        .filter(|workspace| workspace.other_members > 0)
        .map(|workspace| workspace.name.as_str())
        .collect();
    if !shared.is_empty() {
        return Err(ApiError::conflict(
            "owns_shared_workspace",
            format!(
                "You own {} with other members. Transfer ownership to an admin, remove them or delete the workspace first.",
                shared.join(", ")
            ),
        ));
    }

    let sole_admin = db::organizations::sole_admin_of(&state.db, user_id).await?;
    if !sole_admin.is_empty() {
        return Err(ApiError::conflict(
            "last_organization_admin",
            format!(
                "You are the only admin of {}. Make someone else an admin there first.",
                sole_admin.join(", ")
            ),
        ));
    }

    for workspace in &owned {
        crate::privacy::delete_workspace(
            &state,
            workspace.id,
            workspace.stripe_customer_id.as_deref(),
        )
        .await?;
    }
    db::users::delete(&state.db, user_id).await?;
    tracing::info!(%user_id, "Deleted an account");

    Ok((
        StatusCode::NO_CONTENT,
        cookies.remove(expired_cookie(SESSION_COOKIE_NAME)),
    ))
}
