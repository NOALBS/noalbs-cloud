//! Chat grants: streamers letting the bot into their channel, and the bot
//! accounts it speaks as (the shared NOALBS bot, an organization's for all its
//! client workspaces, or a workspace's own on plans with `custom_chat_bot`, for a
//! client workspace when its organization allows it).
//!
//! All of them go through one OAuth flow with its own state cookie and one
//! callback (`/chat/connect/{platform}/callback`); only the scopes and where the
//! result is stored differ. The callback checks the permission again, since the
//! grant can take minutes, then tells the chat service to reconcile.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::get,
};
use axum_extra::extract::cookie::PrivateCookieJar;
use chrono::{DateTime, Duration, Utc};
use db::{chat, organizations::OrganizationPermission, workspaces::WorkspacePermission};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    oauth::{self, LoginPlatform, TokenResponse},
    permissions::{
        AccountManage, AccountView, CurrentUser, OrgManage, OrgView, OrganizationAuthorized, Staff,
        SwitcherAuthorized, SwitcherManage, WorkspaceAuthorized,
    },
    routes::{
        ApiError, ApiResult, ErrorBody,
        auth::{OAuthCallbackQuery, OAuthRedirectQuery, expired_cookie, state_cookie, with_query},
        organization_audit::{record, record_team},
    },
};

const STATE_COOKIE_NAME: &str = "chat_oauth_state";

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(workspace_chat_bots))
        .routes(routes!(remove_workspace_chat_bot))
        .routes(routes!(admin_chat_bots))
        .routes(routes!(admin_remove_chat_bot))
        .routes(routes!(organization_chat_bots))
        .routes(routes!(remove_organization_chat_bot))
        // Browser redirects for the grants; not part of the JSON API.
        .route(
            "/workspaces/{workspace_id}/switchers/{switcher_id}/chat/connect/{platform}",
            get(connect_channel),
        )
        .route(
            "/workspaces/{workspace_id}/chat-bot/connect/{platform}",
            get(connect_workspace_bot),
        )
        .route(
            "/organizations/{organization_id}/chat-bot/connect/{platform}",
            get(connect_organization_bot),
        )
        .route(
            "/admin/chat-bots/connect/{platform}",
            get(connect_shared_bot),
        )
        .route("/chat/connect/{platform}/callback", get(callback))
}

/// A platform with chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChatPlatform {
    Twitch,
    Kick,
    YouTube,
}

impl From<chat::ChatPlatform> for ChatPlatform {
    fn from(platform: chat::ChatPlatform) -> Self {
        match platform {
            chat::ChatPlatform::Twitch => Self::Twitch,
            chat::ChatPlatform::Kick => Self::Kick,
            chat::ChatPlatform::YouTube => Self::YouTube,
        }
    }
}

impl ChatPlatform {
    pub fn db(self) -> chat::ChatPlatform {
        match self {
            Self::Twitch => chat::ChatPlatform::Twitch,
            Self::Kick => chat::ChatPlatform::Kick,
            Self::YouTube => chat::ChatPlatform::YouTube,
        }
    }

    pub fn login(self) -> LoginPlatform {
        match self {
            Self::Twitch => LoginPlatform::Twitch,
            Self::Kick => LoginPlatform::Kick,
            Self::YouTube => LoginPlatform::YouTube,
        }
    }
}

fn db_platform(platform: LoginPlatform) -> chat::ChatPlatform {
    match platform {
        LoginPlatform::Twitch => chat::ChatPlatform::Twitch,
        LoginPlatform::Kick => chat::ChatPlatform::Kick,
        LoginPlatform::YouTube => chat::ChatPlatform::YouTube,
    }
}

/// Whether a grant still works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GrantStatus {
    Active,
    /// The platform refused it: connect it again.
    Revoked,
}

impl From<chat::GrantStatus> for GrantStatus {
    fn from(status: chat::GrantStatus) -> Self {
        match status {
            chat::GrantStatus::Active => Self::Active,
            chat::GrantStatus::Revoked => Self::Revoked,
        }
    }
}

/// A bot account the chat service speaks as.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatBotDto {
    pub platform: ChatPlatform,
    pub login: String,
    pub display_name: String,
    pub status: GrantStatus,
    pub connected_at: DateTime<Utc>,
}

impl From<chat::ChatBot> for ChatBotDto {
    fn from(bot: chat::ChatBot) -> Self {
        Self {
            platform: bot.platform.into(),
            login: bot.login,
            display_name: bot.display_name,
            status: bot.status.into(),
            connected_at: bot.updated_at,
        }
    }
}

/// Who speaks for a workspace in chat.
#[derive(Debug, Serialize, ToSchema)]
pub struct WorkspaceChatBotsDto {
    /// Whether the plan lets the workspace connect its own bot.
    pub custom_allowed: bool,
    /// The workspace's own bots. They speak only while `custom_allowed`.
    pub own: Vec<ChatBotDto>,
    /// A client workspace's organization's bots, which speak where the workspace
    /// has none of its own (empty for other workspaces).
    pub organization: Vec<ChatBotDto>,
    /// The shared NOALBS bots, which speak where nobody else's does.
    pub shared: Vec<ChatBotDto>,
}

/// Get the workspace's chat bots and the shared ones.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/chat-bot", tag = "chat",
    params(("workspace_id" = Uuid, Path)),
    responses((status = OK, body = WorkspaceChatBotsDto), (status = FORBIDDEN, body = ErrorBody)))]
async fn workspace_chat_bots(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<AccountView>,
) -> ApiResult<Json<WorkspaceChatBotsDto>> {
    let workspace_id = authorized.into_access().workspace_id;
    let plan = db::plans::for_workspace(&state.db, workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Workspace not found"))?;
    let workspace = db::workspaces::get_by_id(&state.db, workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Workspace not found"))?;
    let own = chat::bots(&state.db, Some(workspace_id)).await?;
    let organization = match workspace.organization_id {
        Some(organization_id) => chat::organization_bots(&state.db, organization_id).await?,
        None => Vec::new(),
    };
    let shared = chat::bots(&state.db, None).await?;

    Ok(Json(WorkspaceChatBotsDto {
        custom_allowed: plan.custom_chat_bot,
        own: own.into_iter().map(Into::into).collect(),
        organization: organization.into_iter().map(Into::into).collect(),
        shared: shared.into_iter().map(Into::into).collect(),
    }))
}

/// Disconnect the workspace's own bot; the shared bot speaks again.
#[utoipa::path(delete, path = "/workspaces/{workspace_id}/chat-bot/{platform}", tag = "chat",
    params(("workspace_id" = Uuid, Path), ("platform" = ChatPlatform, Path)),
    responses((status = NO_CONTENT, description = "Disconnected"), (status = NOT_FOUND, body = ErrorBody)))]
async fn remove_workspace_chat_bot(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<AccountManage>,
    Path(path): Path<(Uuid, ChatPlatform)>,
) -> ApiResult<StatusCode> {
    let (_, platform) = path;
    let workspace_id = authorized.into_access().workspace_id;
    remove_bot(&state, Some(workspace_id), platform).await
}

/// List an organization's chat bots, which speak for all its client workspaces.
#[utoipa::path(get, path = "/organizations/{organization_id}/chat-bots", tag = "organizations",
    params(("organization_id" = Uuid, Path)),
    responses((status = OK, body = Vec<ChatBotDto>), (status = NOT_FOUND, body = ErrorBody)))]
async fn organization_chat_bots(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgView>,
) -> ApiResult<Json<Vec<ChatBotDto>>> {
    let access = authorized.into_access();
    let bots = chat::organization_bots(&state.db, access.organization_id).await?;
    Ok(Json(bots.into_iter().map(Into::into).collect()))
}

/// Disconnect an organization's bot; its clients' channels get the shared bot.
#[utoipa::path(delete, path = "/organizations/{organization_id}/chat-bots/{platform}", tag = "organizations",
    params(("organization_id" = Uuid, Path), ("platform" = ChatPlatform, Path)),
    responses((status = NO_CONTENT, description = "Disconnected"), (status = NOT_FOUND, body = ErrorBody)))]
async fn remove_organization_chat_bot(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgManage>,
    Path(path): Path<(Uuid, ChatPlatform)>,
) -> ApiResult<StatusCode> {
    let (_, platform) = path;
    let access = authorized.into_access();
    if !chat::remove_organization_bot(&state.db, access.organization_id, platform.db()).await? {
        return Err(ApiError::not_found("No bot is connected"));
    }
    state.notify_chat().await;
    record_team(
        &state,
        &access,
        "chat_bot_removed",
        None,
        serde_json::json!({ "platform": platform }),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// List the shared NOALBS bots.
#[utoipa::path(get, path = "/admin/chat-bots", tag = "admin",
    responses((status = OK, body = Vec<ChatBotDto>), (status = FORBIDDEN, body = ErrorBody)))]
async fn admin_chat_bots(
    State(state): State<ApiState>,
    _staff: Staff,
) -> ApiResult<Json<Vec<ChatBotDto>>> {
    let bots = chat::bots(&state.db, None).await?;
    Ok(Json(bots.into_iter().map(Into::into).collect()))
}

/// Disconnect a shared bot. Channels without a workspace bot go quiet.
#[utoipa::path(delete, path = "/admin/chat-bots/{platform}", tag = "admin",
    params(("platform" = ChatPlatform, Path)),
    responses((status = NO_CONTENT, description = "Disconnected"), (status = NOT_FOUND, body = ErrorBody)))]
async fn admin_remove_chat_bot(
    State(state): State<ApiState>,
    _staff: Staff,
    Path(platform): Path<ChatPlatform>,
) -> ApiResult<StatusCode> {
    remove_bot(&state, None, platform).await
}

async fn remove_bot(
    state: &ApiState,
    workspace_id: Option<Uuid>,
    platform: ChatPlatform,
) -> ApiResult<StatusCode> {
    if !chat::remove_bot(&state.db, workspace_id, platform.db()).await? {
        return Err(ApiError::not_found("No bot is connected"));
    }
    state.notify_chat().await;
    Ok(StatusCode::NO_CONTENT)
}

/// What a grant is for.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Grantee {
    /// A streamer's channel, linked to this switcher.
    Channel { switcher_id: Uuid },
    /// A workspace's own bot account.
    WorkspaceBot { workspace_id: Uuid },
    /// An organization's bot account, for all its client workspaces.
    OrganizationBot { organization_id: Uuid },
    /// The shared NOALBS bot account (staff).
    SharedBot,
}

impl Grantee {
    fn scopes(self, platform: LoginPlatform) -> &'static [&'static str] {
        match (self, platform) {
            (Self::Channel { .. }, LoginPlatform::Twitch) => oauth::twitch::CHANNEL_SCOPES,
            (Self::Channel { .. }, LoginPlatform::Kick) => oauth::kick::CHANNEL_SCOPES,
            (Self::Channel { .. }, LoginPlatform::YouTube) => oauth::youtube::CHANNEL_SCOPES,
            (_, LoginPlatform::Twitch) => oauth::twitch::BOT_SCOPES,
            (_, LoginPlatform::Kick) => oauth::kick::BOT_SCOPES,
            (_, LoginPlatform::YouTube) => oauth::youtube::BOT_SCOPES,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct GrantState {
    state: String,
    code_verifier: String,
    platform: String,
    grantee: Grantee,
    redirect_target: Option<String>,
}

#[derive(Deserialize)]
struct ChannelPath {
    switcher_id: Uuid,
    platform: String,
}

#[derive(Deserialize)]
struct OrganizationPath {
    organization_id: Uuid,
    platform: String,
}

#[derive(Deserialize)]
struct WorkspacePath {
    workspace_id: Uuid,
    platform: String,
}

async fn connect_channel(
    State(state): State<ApiState>,
    _authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<ChannelPath>,
    Query(query): Query<OAuthRedirectQuery>,
    cookies: PrivateCookieJar,
) -> ApiResult<(PrivateCookieJar, Redirect)> {
    let grantee = Grantee::Channel {
        switcher_id: path.switcher_id,
    };
    begin(&state, &path.platform, grantee, query.redirect, cookies)
}

async fn connect_workspace_bot(
    State(state): State<ApiState>,
    _authorized: WorkspaceAuthorized<AccountManage>,
    Path(path): Path<WorkspacePath>,
    Query(query): Query<OAuthRedirectQuery>,
    cookies: PrivateCookieJar,
) -> ApiResult<(PrivateCookieJar, Redirect)> {
    if let Err(error) = ensure_custom_bot_allowed(&state, path.workspace_id).await {
        return Ok((cookies, fail(query.redirect.as_deref(), error)?));
    }
    let grantee = Grantee::WorkspaceBot {
        workspace_id: path.workspace_id,
    };
    begin(&state, &path.platform, grantee, query.redirect, cookies)
}

async fn connect_organization_bot(
    State(state): State<ApiState>,
    _authorized: OrganizationAuthorized<OrgManage>,
    Path(path): Path<OrganizationPath>,
    Query(query): Query<OAuthRedirectQuery>,
    cookies: PrivateCookieJar,
) -> ApiResult<(PrivateCookieJar, Redirect)> {
    let grantee = Grantee::OrganizationBot {
        organization_id: path.organization_id,
    };
    begin(&state, &path.platform, grantee, query.redirect, cookies)
}

async fn connect_shared_bot(
    State(state): State<ApiState>,
    _staff: Staff,
    Path(platform): Path<String>,
    Query(query): Query<OAuthRedirectQuery>,
    cookies: PrivateCookieJar,
) -> ApiResult<(PrivateCookieJar, Redirect)> {
    begin(
        &state,
        &platform,
        Grantee::SharedBot,
        query.redirect,
        cookies,
    )
}

/// Back to the page that started it with `chat_error`, or the error itself
/// without one.
fn fail(redirect: Option<&str>, error: ApiError) -> ApiResult<Redirect> {
    match redirect.and_then(oauth::sanitize_redirect_target) {
        Some(target) => Ok(Redirect::temporary(&with_query(
            &target,
            "chat_error",
            error.code(),
        ))),
        None => Err(error),
    }
}

async fn ensure_custom_bot_allowed(state: &ApiState, workspace_id: Uuid) -> ApiResult<()> {
    let plan = db::plans::for_workspace(&state.db, workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Workspace not found"))?;
    if !plan.custom_chat_bot {
        let managed = db::workspaces::get_by_id(&state.db, workspace_id)
            .await?
            .is_some_and(|workspace| workspace.organization_id.is_some());
        return Err(ApiError::forbidden(
            "plan_limit",
            if managed {
                "Your organization doesn't allow your own chat bot"
            } else {
                "Your plan doesn't include your own chat bot"
            },
        ));
    }
    Ok(())
}

fn begin(
    state: &ApiState,
    platform: &str,
    grantee: Grantee,
    redirect: Option<String>,
    cookies: PrivateCookieJar,
) -> ApiResult<(PrivateCookieJar, Redirect)> {
    let platform: LoginPlatform = platform.parse()?;
    let pkce = oauth::generate_pkce();
    let csrf_state = Uuid::new_v4().to_string();
    let callback_url = state.oauth.chat_callback_url(platform);
    let authorize_url = match state.oauth.authorize_url(
        platform,
        &oauth::AuthorizeRequest {
            redirect_uri: &callback_url,
            state: &csrf_state,
            challenge: &pkce.challenge,
            scopes: &grantee.scopes(platform).join(" "),
            // Twitch would otherwise reuse whichever account the browser is
            // signed in with, which for a bot is usually the wrong one.
            force_verify: true,
        },
    ) {
        Ok(url) => url,
        Err(error) => return Ok((cookies, fail(redirect.as_deref(), error)?)),
    };
    let grant_state = GrantState {
        state: csrf_state,
        code_verifier: pkce.verifier,
        platform: platform.as_str().to_owned(),
        grantee,
        redirect_target: redirect
            .as_deref()
            .and_then(oauth::sanitize_redirect_target),
    };
    let value = serde_json::to_string(&grant_state).map_err(|error| {
        tracing::error!(%error, "Could not serialize the chat grant state");
        ApiError::internal()
    })?;

    Ok((
        cookies.add(state_cookie(STATE_COOKIE_NAME, value)),
        Redirect::temporary(&authorize_url),
    ))
}

async fn callback(
    State(state): State<ApiState>,
    user: CurrentUser,
    Path(platform): Path<String>,
    Query(query): Query<OAuthCallbackQuery>,
    cookies: PrivateCookieJar,
) -> ApiResult<impl IntoResponse> {
    let platform: LoginPlatform = platform.parse()?;
    let grant_state = cookies
        .get(STATE_COOKIE_NAME)
        .and_then(|cookie| serde_json::from_str::<GrantState>(cookie.value()).ok())
        .filter(|saved| {
            saved.platform == platform.as_str() && query.state.as_deref() == Some(&saved.state)
        })
        .ok_or_else(|| ApiError::bad_request("oauth_state", "Invalid OAuth state"))?;
    let cookies = cookies.remove(expired_cookie(STATE_COOKIE_NAME));

    let outcome = match (&query.error, &query.code) {
        (None, Some(code)) => complete(&state, &user, platform, &grant_state, code).await,
        (None, None) => Err(ApiError::bad_request(
            "oauth_missing_code",
            "OAuth callback did not include an authorization code",
        )),
        (Some(_), _) => Err(ApiError::bad_request(
            "oauth_denied",
            "OAuth authorization was denied",
        )),
    };
    let target = grant_state.redirect_target.as_deref().unwrap_or("/");
    let target = match outcome {
        Ok(()) => with_query(target, "chat_connected", platform.as_str()),
        Err(error) => {
            if error.status().is_server_error() {
                tracing::warn!(code = error.code(), "Chat grant failed");
            }
            with_query(target, "chat_error", error.code())
        }
    };

    Ok((cookies, Redirect::temporary(&target)))
}

async fn complete(
    state: &ApiState,
    user: &CurrentUser,
    platform: LoginPlatform,
    grant_state: &GrantState,
    code: &str,
) -> ApiResult<()> {
    let grantee = grant_state.grantee;
    authorize(state, user, grantee).await?;

    let token = state
        .oauth
        .exchange_code(
            &state.http,
            platform,
            code,
            &state.oauth.chat_callback_url(platform),
            &grant_state.code_verifier,
        )
        .await?;
    if !token.has_scopes(grantee.scopes(platform)) {
        return Err(ApiError::bad_request(
            "missing_scopes",
            "Not every permission was granted",
        ));
    }
    // The chat service refreshes YouTube tokens (they last an hour).
    if platform == LoginPlatform::YouTube && token.refresh_token.is_none() {
        return Err(ApiError::bad_request(
            "missing_refresh_token",
            "YouTube gave no refresh token",
        ));
    }
    let profile = state
        .oauth
        .fetch_profile(&state.http, platform, &token.access_token)
        .await?;
    let account = chat::PlatformAccount {
        platform: db_platform(platform),
        id: &profile.id,
        login: &profile.login,
        display_name: &profile.display_name,
    };
    let stored = StoredTokens::new(state, platform, &token);
    let grant = stored.grant();

    match grantee {
        Grantee::Channel { switcher_id } => {
            chat::link_channel(&state.db, switcher_id, account, grant, user.id()).await?;
        }
        Grantee::WorkspaceBot { workspace_id } => {
            chat::upsert_bot(&state.db, Some(workspace_id), account, grant, user.id()).await?;
        }
        Grantee::OrganizationBot { organization_id } => {
            chat::upsert_organization_bot(&state.db, organization_id, account, grant, user.id())
                .await?;
            record(
                state,
                organization_id,
                user.id(),
                db::organization_audit::ActorKind::Team,
                "chat_bot_connected",
                None,
                serde_json::json!({
                    "platform": platform.as_str(),
                    "account": profile.display_name,
                }),
            )
            .await?;
        }
        Grantee::SharedBot => {
            chat::upsert_bot(&state.db, None, account, grant, user.id()).await?;
        }
    }
    state.notify_chat().await;
    Ok(())
}

/// The permission to start the grant, checked again when it comes back.
async fn authorize(state: &ApiState, user: &CurrentUser, grantee: Grantee) -> ApiResult<()> {
    let forbidden = || ApiError::forbidden("forbidden", "You can't connect this any more");
    let (workspace_id, permission) = match grantee {
        Grantee::SharedBot if user.is_staff() => return Ok(()),
        Grantee::SharedBot => return Err(forbidden()),
        Grantee::OrganizationBot { organization_id } => {
            // The organization's team only: staff have no say in its bot.
            let allowed = db::organizations::has_permission(
                &state.db,
                organization_id,
                user.id(),
                OrganizationPermission::MANAGE,
            )
            .await?;
            return if allowed { Ok(()) } else { Err(forbidden()) };
        }
        Grantee::Channel { switcher_id } => {
            let switcher = db::switchers::get(&state.db, switcher_id)
                .await?
                .ok_or_else(|| ApiError::not_found("Switcher not found"))?;
            (switcher.workspace_id, WorkspacePermission::SWITCHER_MANAGE)
        }
        Grantee::WorkspaceBot { workspace_id } => {
            ensure_custom_bot_allowed(state, workspace_id).await?;
            (workspace_id, WorkspacePermission::ACCOUNT_MANAGE)
        }
    };
    // Members only: linking a channel or a bot is the workspace's own call, so
    // NOALBS support never does it for them.
    let allowed =
        db::workspaces::has_permission(&state.db, workspace_id, user.id(), permission).await?;
    if !allowed {
        return Err(forbidden());
    }
    Ok(())
}

/// The tokens we keep, encrypted. Twitch keeps its grants itself (the chat
/// service works with an app token), so only Kick's and YouTube's are stored.
struct StoredTokens {
    access_token: Option<Vec<u8>>,
    refresh_token: Option<Vec<u8>>,
    expires_at: Option<DateTime<Utc>>,
    scopes: Vec<String>,
}

impl StoredTokens {
    fn new(state: &ApiState, platform: LoginPlatform, token: &TokenResponse) -> Self {
        let scopes = token.scopes();
        if platform == LoginPlatform::Twitch {
            return Self {
                access_token: None,
                refresh_token: None,
                expires_at: None,
                scopes,
            };
        }
        Self {
            access_token: Some(state.data_key.encrypt(token.access_token.as_bytes())),
            refresh_token: token
                .refresh_token
                .as_ref()
                .map(|refresh| state.data_key.encrypt(refresh.as_bytes())),
            expires_at: token
                .expires_in
                .filter(|seconds| *seconds >= 0)
                .map(|seconds| Utc::now() + Duration::seconds(seconds)),
            scopes,
        }
    }

    fn grant(&self) -> chat::Grant<'_> {
        chat::Grant {
            access_token: self.access_token.as_deref(),
            refresh_token: self.refresh_token.as_deref(),
            expires_at: self.expires_at,
            scopes: &self.scopes,
        }
    }
}
