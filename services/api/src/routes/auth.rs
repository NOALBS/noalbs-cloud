use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Redirect},
    routing::get,
};
use axum_extra::extract::cookie::{Cookie, PrivateCookieJar, SameSite};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    oauth::{
        self, LoginPlatform, OAUTH_STATE_COOKIE_NAME, OAuthFlow, OAuthStateCookie, TokenResponse,
    },
    permissions::{CurrentUser, SESSION_COOKIE_NAME},
    routes::{ApiError, ApiResult, ErrorBody},
    validate,
};

/// How long a new session lasts: it's extended each time it's used
/// (`db::sessions::IDLE_LIFETIME` without use ends it).
const SESSION_LIFETIME: Duration = db::sessions::IDLE_LIFETIME;
/// The session cookie outlives the session (400 days is the most browsers
/// keep); the server decides whether the session behind it is still valid.
const SESSION_COOKIE_MAX_AGE: time::Duration = time::Duration::days(400);
const OAUTH_STATE_LIFETIME: time::Duration = time::Duration::minutes(10);

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(me))
        .routes(routes!(providers))
        .routes(routes!(logout))
        .routes(routes!(dev_login))
        // Browser redirects for the OAuth flows; not part of the JSON API.
        .route("/auth/{platform}", get(login_redirect))
        .route("/auth/{platform}/callback", get(login_callback))
        .route("/auth/link/{platform}", get(link_redirect))
        .route("/auth/link/{platform}/callback", get(link_callback))
}

/// The signed-in user.
#[derive(Debug, Serialize, ToSchema)]
pub struct MeDto {
    pub id: Uuid,
    pub staff: bool,
    /// The workspace to open by default, if the user is still a member of it.
    pub last_workspace_slug: Option<String>,
    /// Linked login accounts, most recently used first. The first one is the
    /// profile to show for this user.
    pub connections: Vec<ConnectionDto>,
}

/// A linked login account.
#[derive(Debug, Serialize, ToSchema)]
pub struct ConnectionDto {
    pub id: Uuid,
    /// `TWITCH`, `KICK`, `YOUTUBE`, `DISCORD` or `DEV`.
    pub provider: String,
    pub username: String,
    pub avatar_url: Option<String>,
    pub connected_at: chrono::DateTime<Utc>,
}

async fn me_dto(state: &ApiState, user: &db::users::User) -> ApiResult<MeDto> {
    let last_workspace_slug = db::users::last_workspace_slug(&state.db, user.id).await?;
    let connections = db::oauth2_connections::list_profiles_for_user(&state.db, user.id)
        .await?
        .into_iter()
        .map(|connection| ConnectionDto {
            id: connection.id,
            provider: connection.provider,
            username: connection.provider_username,
            avatar_url: connection.provider_avatar_url,
            connected_at: connection.created_at,
        })
        .collect();

    Ok(MeDto {
        id: user.id,
        staff: user.staff,
        last_workspace_slug,
        connections,
    })
}

/// Get the signed-in user.
#[utoipa::path(get, path = "/auth/me", tag = "auth",
    responses((status = OK, body = MeDto), (status = UNAUTHORIZED, body = ErrorBody)))]
async fn me(
    State(state): State<ApiState>,
    headers: HeaderMap,
    current_user: Result<CurrentUser, ApiError>,
) -> axum::response::Response {
    let error = match current_user {
        Ok(user) => {
            return match me_dto(&state, &user.0).await {
                Ok(me) => Json(me).into_response(),
                Err(error) => error.into_response(),
            };
        }
        Err(error) => error,
    };
    // A session cookie that no longer signs anyone in (expired, signed out
    // elsewhere, deleted, or sealed with another secret): remove it, or Caddy
    // keeps serving the app at `/` instead of the landing page. Checked on the raw
    // header, since an unreadable cookie looks absent in the private jar.
    let stale = error.status() == StatusCode::UNAUTHORIZED
        && headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(';'))
            .any(|pair| {
                pair.trim_start()
                    .starts_with(&format!("{SESSION_COOKIE_NAME}="))
            });
    if stale {
        // Written as a header: the jar only removes cookies it could decrypt.
        (
            [(
                header::SET_COOKIE,
                expired_cookie(SESSION_COOKIE_NAME).to_string(),
            )],
            error,
        )
            .into_response()
    } else {
        error.into_response()
    }
}

/// Which ways to sign in are available.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProvidersDto {
    pub twitch: bool,
    pub kick: bool,
    pub youtube: bool,
    /// Whether `POST /auth/dev-login` is enabled (local development only).
    pub dev_login: bool,
}

/// List the available sign-in methods.
#[utoipa::path(get, path = "/auth/providers", tag = "auth",
    responses((status = OK, body = ProvidersDto)))]
async fn providers(State(state): State<ApiState>) -> Json<ProvidersDto> {
    Json(ProvidersDto {
        twitch: state.oauth.is_configured(LoginPlatform::Twitch),
        kick: state.oauth.is_configured(LoginPlatform::Kick),
        youtube: state.oauth.is_configured(LoginPlatform::YouTube),
        dev_login: state.dev_login,
    })
}

/// Sign out and clear the session cookie.
#[utoipa::path(post, path = "/auth/logout", tag = "auth",
    responses((status = OK, description = "Signed out")))]
async fn logout(
    State(state): State<ApiState>,
    cookies: PrivateCookieJar,
) -> ApiResult<impl IntoResponse> {
    if let Some(cookie) = cookies.get(SESSION_COOKIE_NAME)
        && let Ok(session_id) = Uuid::parse_str(cookie.value())
        && let Err(error) = db::sessions::delete(&state.db, session_id).await
    {
        tracing::warn!(%error, "Failed to delete session during logout");
    }

    Ok(cookies.remove(expired_cookie(SESSION_COOKIE_NAME)))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DevLogin {
    /// Any name; the same name signs in as the same user again.
    pub username: String,
}

/// Sign in without OAuth. Only available when the API runs with
/// `dev_login = true`; answers 404 otherwise.
#[utoipa::path(post, path = "/auth/dev-login", tag = "auth",
    request_body = DevLogin,
    responses((status = OK, body = MeDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn dev_login(
    State(state): State<ApiState>,
    headers: HeaderMap,
    cookies: PrivateCookieJar,
    Json(request): Json<DevLogin>,
) -> ApiResult<(PrivateCookieJar, Json<MeDto>)> {
    if !state.dev_login {
        return Err(ApiError::not_found("Not found"));
    }

    let username = validate::name(&request.username)?;
    let provider = db::oauth2_connections::OAuth2Provider::Dev;
    let provider_id = username.to_lowercase();

    let existing =
        db::oauth2_connections::get_by_provider_id(&state.db, provider, &provider_id).await?;
    let user_id = match existing {
        Some(connection) => connection.user_id,
        None => db::users::create(&state.db).await?.id,
    };
    if is_disabled(&state, user_id).await? {
        return Err(account_disabled());
    }

    let connection = db::oauth2_connections::upsert_for_user(
        &state.db,
        user_id,
        db::oauth2_connections::UpsertOAuth2Connection {
            provider,
            access_token: "dev",
            refresh_token: None,
            token_type: None,
            expires_at: None,
            scopes: &[],
            provider_id: &provider_id,
            provider_username: username,
            provider_email: None,
            provider_avatar_url: None,
        },
    )
    .await?
    .ok_or_else(ApiError::internal)?;

    let session = db::sessions::create(
        &state.db,
        connection.id,
        Utc::now() + SESSION_LIFETIME,
        user_agent(&headers),
    )
    .await?;
    let user = db::users::get_by_id(&state.db, user_id)
        .await?
        .ok_or_else(ApiError::internal)?;

    let me = me_dto(&state, &user).await?;

    Ok((cookies.add(session_cookie(session.id)), Json(me)))
}

#[derive(Deserialize)]
pub(crate) struct OAuthRedirectQuery {
    pub redirect: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct OAuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

async fn login_redirect(
    State(state): State<ApiState>,
    Path(platform): Path<String>,
    Query(query): Query<OAuthRedirectQuery>,
    cookies: PrivateCookieJar,
) -> ApiResult<impl IntoResponse> {
    let platform = platform.parse()?;
    begin_oauth(
        &state,
        platform,
        OAuthFlow::Login,
        query.redirect.as_deref(),
        cookies,
    )
}

async fn link_redirect(
    _current_user: CurrentUser,
    State(state): State<ApiState>,
    Path(platform): Path<String>,
    Query(query): Query<OAuthRedirectQuery>,
    cookies: PrivateCookieJar,
) -> ApiResult<impl IntoResponse> {
    let platform = platform.parse()?;
    begin_oauth(
        &state,
        platform,
        OAuthFlow::Link,
        query.redirect.as_deref(),
        cookies,
    )
}

async fn login_callback(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(platform): Path<String>,
    Query(query): Query<OAuthCallbackQuery>,
    cookies: PrivateCookieJar,
) -> ApiResult<impl IntoResponse> {
    let platform = platform.parse()?;
    let oauth_state = validate_oauth_callback(&cookies, &query, OAuthFlow::Login)?;
    let connection = match exchange_and_store(&state, platform, &query, &oauth_state, None).await {
        // A Google account without a YouTube channel: say so on the sign-in page.
        Err(error) if error.code() == "no_youtube_channel" => {
            let cookies = cookies.remove(expired_cookie(OAUTH_STATE_COOKIE_NAME));
            return Ok((
                cookies,
                Redirect::temporary("/login?error=no_youtube_channel"),
            ));
        }
        result => result?,
    };
    if is_disabled(&state, connection.user_id).await? {
        let cookies = cookies.remove(expired_cookie(OAUTH_STATE_COOKIE_NAME));
        return Ok((
            cookies,
            Redirect::temporary("/login?error=account_disabled"),
        ));
    }
    let session = db::sessions::create(
        &state.db,
        connection.id,
        Utc::now() + SESSION_LIFETIME,
        user_agent(&headers),
    )
    .await?;
    let cookies = cookies
        .remove(expired_cookie(OAUTH_STATE_COOKIE_NAME))
        .add(session_cookie(session.id));

    Ok((cookies, Redirect::temporary(redirect_target(&oauth_state))))
}

async fn link_callback(
    current_user: CurrentUser,
    State(state): State<ApiState>,
    Path(platform): Path<String>,
    Query(query): Query<OAuthCallbackQuery>,
    cookies: PrivateCookieJar,
) -> ApiResult<impl IntoResponse> {
    let platform: LoginPlatform = platform.parse()?;
    let oauth_state = oauth_state_from_cookie(&cookies, &query, OAuthFlow::Link)?;
    // Back to the page that started it, with the outcome for it to show.
    let outcome = match query.error {
        Some(_) => Err(ApiError::bad_request(
            "oauth_denied",
            "OAuth authorization was denied",
        )),
        None => {
            exchange_and_store(
                &state,
                platform,
                &query,
                &oauth_state,
                Some(current_user.id()),
            )
            .await
        }
    };
    let target = match outcome {
        Ok(_) => with_query(redirect_target(&oauth_state), "linked", platform.as_str()),
        Err(error) => with_query(redirect_target(&oauth_state), "link_error", error.code()),
    };
    let cookies = cookies.remove(expired_cookie(OAUTH_STATE_COOKIE_NAME));

    Ok((cookies, Redirect::temporary(&target)))
}

fn begin_oauth(
    state: &ApiState,
    platform: LoginPlatform,
    flow: OAuthFlow,
    redirect: Option<&str>,
    cookies: PrivateCookieJar,
) -> Result<(PrivateCookieJar, Redirect), ApiError> {
    state.oauth.provider(platform)?;
    let pkce = oauth::generate_pkce();
    let csrf_state = Uuid::new_v4().to_string();
    let oauth_state = OAuthStateCookie {
        state: csrf_state.clone(),
        code_verifier: pkce.verifier,
        flow,
        redirect_target: redirect.and_then(oauth::sanitize_redirect_target),
    };
    let cookie_value = serde_json::to_string(&oauth_state).map_err(|error| {
        tracing::error!(%error, "Could not serialize OAuth state");
        ApiError::internal()
    })?;
    let cookies = cookies.add(state_cookie(OAUTH_STATE_COOKIE_NAME, cookie_value));
    let callback_url = match flow {
        OAuthFlow::Login => state.oauth.callback_url(platform),
        OAuthFlow::Link => state.oauth.link_callback_url(platform),
    };
    let scopes = match platform {
        LoginPlatform::Twitch => oauth::twitch::LOGIN_SCOPES,
        LoginPlatform::Kick => oauth::kick::LOGIN_SCOPES,
        LoginPlatform::YouTube => oauth::youtube::LOGIN_SCOPES,
    };
    let authorize_url = state.oauth.authorize_url(
        platform,
        &oauth::AuthorizeRequest {
            redirect_uri: &callback_url,
            state: &csrf_state,
            challenge: &pkce.challenge,
            scopes,
            force_verify: false,
        },
    )?;

    Ok((cookies, Redirect::temporary(&authorize_url)))
}

async fn exchange_and_store(
    state: &ApiState,
    platform: LoginPlatform,
    query: &OAuthCallbackQuery,
    oauth_state: &OAuthStateCookie,
    link_user_id: Option<Uuid>,
) -> Result<db::oauth2_connections::OAuth2Connection, ApiError> {
    let code = query.code.as_deref().ok_or_else(|| {
        ApiError::bad_request(
            "oauth_missing_code",
            "OAuth callback did not include an authorization code",
        )
    })?;
    let callback_url = match oauth_state.flow {
        OAuthFlow::Login => state.oauth.callback_url(platform),
        OAuthFlow::Link => state.oauth.link_callback_url(platform),
    };
    let token = state
        .oauth
        .exchange_code(
            &state.http,
            platform,
            code,
            &callback_url,
            &oauth_state.code_verifier,
        )
        .await?;
    let profile = state
        .oauth
        .fetch_profile(&state.http, platform, &token.access_token)
        .await?;

    let existing =
        db::oauth2_connections::get_by_provider_id(&state.db, platform.db_provider(), &profile.id)
            .await?;
    let user_id = match (link_user_id, existing.as_ref()) {
        (Some(link_user_id), Some(connection)) if connection.user_id != link_user_id => {
            return Err(ApiError::conflict(
                "account_already_linked",
                "This OAuth account is already linked to another user",
            ));
        }
        (Some(link_user_id), _) => link_user_id,
        (None, Some(connection)) => connection.user_id,
        (None, None) => db::users::create(&state.db).await?.id,
    };

    let connection = save_connection(&state.db, user_id, platform, &token, profile).await?;
    connection.ok_or_else(|| {
        ApiError::conflict(
            "account_already_linked",
            "This OAuth account is already linked to another user",
        )
    })
}

async fn save_connection(
    pool: &db::DbPool,
    user_id: Uuid,
    platform: LoginPlatform,
    token: &TokenResponse,
    profile: oauth::Profile,
) -> Result<Option<db::oauth2_connections::OAuth2Connection>, ApiError> {
    let scopes = token.scopes();
    db::oauth2_connections::upsert_for_user(
        pool,
        user_id,
        db::oauth2_connections::UpsertOAuth2Connection {
            provider: platform.db_provider(),
            // Sign-in only needs to know who you are, so its tokens aren't kept
            // (data minimisation). Chat grants live in their own tables.
            access_token: "",
            refresh_token: None,
            token_type: token.token_type.as_deref(),
            expires_at: None,
            scopes: &scopes,
            provider_id: &profile.id,
            provider_username: &profile.display_name,
            // Not asked for or kept (data minimisation); the column stays empty.
            provider_email: None,
            provider_avatar_url: profile.avatar_url.as_deref(),
        },
    )
    .await
    .map_err(ApiError::from)
}

fn validate_oauth_callback(
    cookies: &PrivateCookieJar,
    query: &OAuthCallbackQuery,
    expected_flow: OAuthFlow,
) -> Result<OAuthStateCookie, ApiError> {
    let oauth_state = oauth_state_from_cookie(cookies, query, expected_flow)?;
    if query.error.is_some() {
        return Err(ApiError::bad_request(
            "oauth_denied",
            "OAuth authorization was denied",
        ));
    }

    Ok(oauth_state)
}

/// The OAuth state saved when the flow began, checked against the callback.
fn oauth_state_from_cookie(
    cookies: &PrivateCookieJar,
    query: &OAuthCallbackQuery,
    expected_flow: OAuthFlow,
) -> Result<OAuthStateCookie, ApiError> {
    let cookie = cookies
        .get(OAUTH_STATE_COOKIE_NAME)
        .ok_or_else(|| ApiError::bad_request("oauth_state", "Missing OAuth state cookie"))?;
    let oauth_state = serde_json::from_str::<OAuthStateCookie>(cookie.value())
        .map_err(|_| ApiError::bad_request("oauth_state", "Invalid OAuth state cookie"))?;
    if oauth_state.flow != expected_flow || query.state.as_deref() != Some(&oauth_state.state) {
        return Err(ApiError::bad_request("oauth_state", "Invalid OAuth state"));
    }

    Ok(oauth_state)
}

fn session_cookie(session_id: Uuid) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE_NAME, session_id.to_string()))
        .http_only(true)
        .secure(!cfg!(debug_assertions))
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(SESSION_COOKIE_MAX_AGE)
        .build()
}

/// Holds an OAuth flow's state (CSRF token, PKCE verifier) until the callback.
pub(crate) fn state_cookie(name: &'static str, value: String) -> Cookie<'static> {
    Cookie::build((name, value))
        .http_only(true)
        .secure(!cfg!(debug_assertions))
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(OAUTH_STATE_LIFETIME)
        .build()
}

pub(crate) fn expired_cookie(name: &'static str) -> Cookie<'static> {
    Cookie::build((name, ""))
        .http_only(true)
        .secure(!cfg!(debug_assertions))
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::ZERO)
        .build()
}

/// Whether staff disabled the account (it can't sign in).
async fn is_disabled(state: &ApiState, user_id: Uuid) -> ApiResult<bool> {
    Ok(db::users::get_by_id(&state.db, user_id)
        .await?
        .is_some_and(|user| user.disabled_at.is_some()))
}

fn account_disabled() -> ApiError {
    ApiError::forbidden(
        "account_disabled",
        "This account has been disabled. Contact support@noalbs.com.",
    )
}

fn redirect_target(oauth_state: &OAuthStateCookie) -> &str {
    oauth_state.redirect_target.as_deref().unwrap_or("/")
}

/// Adds `key=value` to a relative URL. Values are codes and platform names, so no escaping.
pub(crate) fn with_query(target: &str, key: &str, value: &str) -> String {
    let separator = if target.contains('?') { '&' } else { '?' };
    format!("{target}{separator}{key}={value}")
}

fn user_agent(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
}

#[cfg(test)]
mod tests {
    use super::{redirect_target, with_query};
    use crate::oauth::{OAuthFlow, OAuthStateCookie, sanitize_redirect_target};

    #[test]
    fn link_outcomes_are_added_to_the_return_url() {
        assert_eq!(
            with_query("/nomad-live/account", "linked", "kick"),
            "/nomad-live/account?linked=kick"
        );
        assert_eq!(
            with_query("/a?tab=1", "link_error", "account_already_linked"),
            "/a?tab=1&link_error=account_already_linked"
        );
    }

    #[test]
    fn only_relative_post_login_redirects_are_accepted() {
        assert_eq!(
            sanitize_redirect_target("/settings"),
            Some("/settings".into())
        );
        assert_eq!(sanitize_redirect_target("https://attacker.example"), None);
        assert_eq!(sanitize_redirect_target("//attacker.example"), None);
        assert_eq!(sanitize_redirect_target("/\\attacker.example"), None);
    }

    #[test]
    fn never_redirects_back_into_a_sign_in_or_off_the_app() {
        // Returning to the sign-in URL would start another sign-in.
        assert_eq!(
            sanitize_redirect_target("/api/auth/twitch?redirect=%2F"),
            None
        );
        assert_eq!(sanitize_redirect_target("/API/auth/kick"), None);
        assert_eq!(sanitize_redirect_target("/login?redirect=/x"), None);
        assert_eq!(sanitize_redirect_target("/source/#token"), None);
        assert_eq!(sanitize_redirect_target("/landing.html"), None);
        assert_eq!(
            sanitize_redirect_target("/nomad-live/s/abc/chat?chat_connected=twitch"),
            Some("/nomad-live/s/abc/chat?chat_connected=twitch".into())
        );
        assert_eq!(sanitize_redirect_target("/apiary"), Some("/apiary".into()));
    }

    #[test]
    fn post_login_redirect_defaults_to_root() {
        let state = OAuthStateCookie {
            state: "state".into(),
            code_verifier: "verifier".into(),
            flow: OAuthFlow::Login,
            redirect_target: None,
        };
        assert_eq!(redirect_target(&state), "/");
    }
}
