use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use axum_extra::extract::cookie::PrivateCookieJar;
use chrono::{DateTime, Utc};
use db::oauth2_connections::UnlinkOutcome;
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use tracing::Instrument;
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
        .routes(routes!(export_history))
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

/// "Download my data" (GDPR access and portability): everything NOALBS Cloud
/// keeps about you but your stream history, which is `/me/export/history`.
#[derive(Debug, Serialize, ToSchema)]
pub struct DataExport {
    pub exported_at: DateTime<Utc>,
    /// What's in the file, what's left out and why.
    pub notes: Vec<String>,
    #[serde(flatten)]
    pub data: db::export::AccountExport,
}

/// Download everything NOALBS Cloud keeps about you, as JSON.
#[utoipa::path(get, path = "/me/export", tag = "auth",
    responses(
        (status = OK, body = DataExport),
        (status = TOO_MANY_REQUESTS, body = ErrorBody),
    ))]
async fn export_data(
    State(state): State<ApiState>,
    current_user: CurrentUser,
) -> ApiResult<impl IntoResponse> {
    let user_id = current_user.id();
    state.limits.export(user_id)?;
    let data = db::export::account(&state.db, user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Account not found"))?;
    let export = DataExport {
        exported_at: Utc::now(),
        notes: vec![
            format!(
                "Everything NOALBS Cloud keeps about you, except what's listed here. What we keep, why and for how long: {}/privacy",
                state.dashboard_url
            ),
            "Stream history, stats, markers and the activity log of your own switchers are a separate download (Account → Download stream history), as they can be large.".to_owned(),
            "This file holds your stream servers' stats URLs and stream keys: keep it private.".to_owned(),
            "Left out: the passwords and API keys of your OBS, Moblin and stream servers, your BELABOX remote key and chat tokens. They're stored encrypted, and you set them yourself. Sign-in tokens and email addresses aren't kept.".to_owned(),
            "Switchers and their settings are listed for your own workspaces: those you own, and those an organization manages for you. Other workspaces belong to their owners, and other people's data, such as who else is in a workspace, is left out.".to_owned(),
            "Activity logs and markers in other people's workspaces record your display name, not your account, so we can't reliably find them for you. Our server logs are kept 14 to 30 days and name accounts by ID. If you think we hold something else about you, email support@noalbs.com and tell us where to look.".to_owned(),
        ],
        data,
    };
    Ok((
        [(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"noalbs-cloud-data.json\"",
        )],
        Json(export),
    ))
}

/// Download the stream history of your own switchers (see `DataExport`), one
/// JSON object per line: an `about` line, then each `stream`, `stats` bucket
/// and activity log `event`, as stored.
#[utoipa::path(get, path = "/me/export/history", tag = "auth",
    responses(
        (status = OK, description = "One JSON object per line", content_type = "application/x-ndjson", body = String),
        (status = TOO_MANY_REQUESTS, body = ErrorBody),
    ))]
async fn export_history(
    State(state): State<ApiState>,
    current_user: CurrentUser,
) -> ApiResult<impl IntoResponse> {
    let user_id = current_user.id();
    state.limits.export(user_id)?;
    let about = serde_json::json!({
        "type": "about",
        "exported_at": Utc::now(),
        "notes": [
            "The stream history of your own switchers (in workspaces you own, or an organization manages for you), as stored, one JSON object per line.",
            "stream: a broadcast, with its stream servers (sources) and markers. stats: one stream server over 10 seconds (sums and counts of the bitrate and latency, minimum and maximum, seconds live, low and offline) and raw, the server's last report, which can hold stream names and IP addresses. event: the activity log, with who did what.",
            format!(
                "Kept as long as your plan's stream history, the activity log at least 30 days: {}/privacy",
                state.dashboard_url
            ),
        ],
    });

    // The body has to own its stream, so the lines are read on a task of their
    // own: a slow download fills the channel and holds the task back, a
    // cancelled one closes it and stops the task.
    let (lines, mut received) = tokio::sync::mpsc::channel(64);
    let pool = state.db.clone();
    tokio::spawn(
        async move {
            let mut history = std::pin::pin!(db::export::history(&pool, user_id));
            while let Some(line) = history.next().await {
                let failed = line.is_err();
                if lines.send(line).await.is_err() || failed {
                    break;
                }
            }
        }
        .in_current_span(),
    );
    let body = stream::once(async move { Ok(about.to_string()) })
        .chain(stream::poll_fn(move |cx| received.poll_recv(cx)))
        .map(move |line: Result<String, db::Error>| match line {
            Ok(line) => Ok(line + "\n"),
            Err(error) => {
                // The download ends broken, so the browser shows it failed.
                tracing::warn!(%error, %user_id, "Couldn't export stream history");
                Err(error)
            }
        });

    Ok((
        [
            (header::CONTENT_TYPE, "application/x-ndjson"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"noalbs-cloud-history.ndjson\"",
            ),
        ],
        Body::from_stream(body),
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
