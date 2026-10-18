use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use db::{
    stream_servers::{Details, StreamServer, StreamServerKind},
    workspaces::WorkspacePermission,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{SwitcherAuthorized, SwitcherManage, SwitcherView},
    routes::{ApiError, ApiResult, ErrorBody},
    server_details::{self, DetailsDto, DetailsInput},
    stats_probe::{self, StatsProbe},
    validate,
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_stream_servers, create_stream_server))
        .routes(routes!(reorder_stream_servers))
        .routes(routes!(update_stream_server, delete_stream_server))
        .routes(routes!(set_overrides))
        .routes(routes!(test_stream_server))
}

const MAX_URL_LEN: usize = 2048;

/// The kind of ingest server, which decides how its stats are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StreamServerKindDto {
    Belabox,
    Mediamtx,
    Sls,
    Nginx,
    Nms,
    Nimble,
    Xiu,
    Openirl,
    Openrtmp,
    Rist,
    Irlhosting,
    IrlhostingWebsocket,
}

impl From<StreamServerKind> for StreamServerKindDto {
    fn from(kind: StreamServerKind) -> Self {
        match kind {
            StreamServerKind::Belabox => Self::Belabox,
            StreamServerKind::Mediamtx => Self::Mediamtx,
            StreamServerKind::Sls => Self::Sls,
            StreamServerKind::Nginx => Self::Nginx,
            StreamServerKind::Nms => Self::Nms,
            StreamServerKind::Nimble => Self::Nimble,
            StreamServerKind::Xiu => Self::Xiu,
            StreamServerKind::Openirl => Self::Openirl,
            StreamServerKind::Openrtmp => Self::Openrtmp,
            StreamServerKind::Rist => Self::Rist,
            StreamServerKind::Irlhosting => Self::Irlhosting,
            StreamServerKind::IrlhostingWebsocket => Self::IrlhostingWebsocket,
        }
    }
}

impl From<StreamServerKindDto> for StreamServerKind {
    fn from(kind: StreamServerKindDto) -> Self {
        match kind {
            StreamServerKindDto::Belabox => Self::Belabox,
            StreamServerKindDto::Mediamtx => Self::Mediamtx,
            StreamServerKindDto::Sls => Self::Sls,
            StreamServerKindDto::Nginx => Self::Nginx,
            StreamServerKindDto::Nms => Self::Nms,
            StreamServerKindDto::Nimble => Self::Nimble,
            StreamServerKindDto::Xiu => Self::Xiu,
            StreamServerKindDto::Openirl => Self::Openirl,
            StreamServerKindDto::Openrtmp => Self::Openrtmp,
            StreamServerKindDto::Rist => Self::Rist,
            StreamServerKindDto::Irlhosting => Self::Irlhosting,
            StreamServerKindDto::IrlhostingWebsocket => Self::IrlhostingWebsocket,
        }
    }
}

/// Scenes to use instead of the switcher's while this server is the one in use.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub struct OverridesDto {
    pub live: Option<Uuid>,
    pub low: Option<Uuid>,
    pub offline: Option<Uuid>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StreamServerDto {
    pub id: Uuid,
    pub name: String,
    pub kind: StreamServerKindDto,
    /// 0 is tried first.
    pub priority: i32,
    pub enabled: bool,
    /// Only shown to people who can manage the switcher and to NOALBS support; it
    /// can contain a key.
    pub stats_url: Option<String>,
    /// Kind-specific settings (no secrets). Only for people who can manage the
    /// switcher or support, like `stats_url`: they can include a stream key.
    pub details: Option<DetailsDto>,
    pub overrides: OverridesDto,
}

impl StreamServerDto {
    fn new(server: StreamServer, show_stats_url: bool) -> Self {
        let details = show_stats_url.then(|| server.details().into());
        Self {
            details,
            id: server.id,
            name: server.name,
            kind: server.kind.into(),
            priority: server.priority,
            enabled: server.enabled,
            stats_url: show_stats_url.then_some(server.stats_url),
            overrides: OverridesDto {
                live: server.override_scene_live,
                low: server.override_scene_low,
                offline: server.override_scene_offline,
            },
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateStreamServer {
    pub name: String,
    pub kind: StreamServerKindDto,
    pub stats_url: String,
    #[serde(default)]
    pub details: DetailsInput,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateStreamServer {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub stats_url: Option<String>,
    /// Replaces the kind-specific settings; secrets left out are kept.
    pub details: Option<DetailsInput>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReorderStreamServers {
    /// Every stream server of the switcher, in the order to try them.
    pub ids: Vec<Uuid>,
}

fn validate_stats_url(kind: StreamServerKind, url: &str, allow_private: bool) -> ApiResult<&str> {
    let url = url.trim();
    let websocket = stats_probe::is_websocket(kind.into());
    let parsed = reqwest::Url::parse(url).ok();
    // IP addresses can be judged now; names are checked every time they're looked up.
    if !allow_private
        && parsed
            .as_ref()
            .and_then(|parsed| parsed.host_str())
            .is_some_and(common::net::is_private_literal)
    {
        return Err(ApiError::bad_request(
            "invalid_stats_url",
            "That address is on a private network, so NOALBS Cloud can't reach it. \
             Use an address that's reachable from the internet.",
        ));
    }
    let valid = url.len() <= MAX_URL_LEN
        && parsed.is_some_and(|parsed| {
            let scheme_ok = if websocket {
                matches!(parsed.scheme(), "ws" | "wss")
            } else {
                matches!(parsed.scheme(), "http" | "https")
            };
            scheme_ok && parsed.has_host()
        });

    if valid {
        Ok(url)
    } else if websocket {
        Err(ApiError::bad_request(
            "invalid_stats_url",
            "Enter the full ws:// or wss:// address of the stats WebSocket",
        ))
    } else {
        Err(ApiError::bad_request(
            "invalid_stats_url",
            "Enter the full http:// or https:// address of the stats page",
        ))
    }
}

fn name_conflict(error: db::Error) -> ApiError {
    if error.constraint() == Some("stream_servers_switcher_id_name_key") {
        ApiError::conflict(
            "name_taken",
            "This switcher already has a stream server with that name",
        )
    } else {
        error.into()
    }
}

/// Whether the server keeps a password or API key we send with every request.
fn has_stored_secrets(details: &Details) -> bool {
    details.encrypted_password.is_some() || details.encrypted_api_key.is_some()
}

async fn server_in_switcher(
    state: &ApiState,
    switcher_id: Uuid,
    stream_server_id: Uuid,
) -> ApiResult<StreamServer> {
    db::stream_servers::get_in_switcher(&state.db, switcher_id, stream_server_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Stream server not found"))
}

/// List a switcher's stream servers in priority order.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers", tag = "stream servers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    responses((status = OK, body = Vec<StreamServerDto>), (status = FORBIDDEN, body = ErrorBody)))]
async fn list_stream_servers(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
) -> ApiResult<Json<Vec<StreamServerDto>>> {
    let access = authorized.into_access();
    // Support at any level: a mistyped stats URL is what they're most often
    // asked about, and a view-only grant should be enough to spot it.
    let show_stats_url = access.workspace.is_support()
        || access
            .workspace
            .can(&state, WorkspacePermission::SWITCHER_MANAGE)
            .await?;
    let servers = db::stream_servers::by_switcher_id(&state.db, access.switcher.id).await?;

    Ok(Json(
        servers
            .into_iter()
            .map(|server| StreamServerDto::new(server, show_stats_url))
            .collect(),
    ))
}

/// Add a stream server at the end of the priority order.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers", tag = "stream servers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = CreateStreamServer,
    responses(
        (status = CREATED, body = StreamServerDto),
        (status = BAD_REQUEST, description = "`invalid_stats_url`, or a missing/invalid kind-specific field (`invalid_key`, …)", body = ErrorBody),
        (status = FORBIDDEN, description = "`plan_limit` when the switcher has its plan's number of stream servers", body = ErrorBody),
        (status = CONFLICT, body = ErrorBody),
    ))]
async fn create_stream_server(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<CreateStreamServer>,
) -> ApiResult<(StatusCode, Json<StreamServerDto>)> {
    let access = authorized.into_access();
    let name = validate::name(&request.name)?;
    let kind = request.kind.into();
    let stats_url = validate_stats_url(kind, &request.stats_url, state.allow_private_stats_urls)?;
    let details =
        server_details::build(kind, request.details, &Details::default(), &state.data_key)?;

    let server = db::stream_servers::create_within_plan(
        &state.db,
        access.switcher.id,
        name,
        kind,
        stats_url,
        &details,
    )
    .await
    .map_err(name_conflict)?
    .ok_or_else(|| {
        ApiError::forbidden(
            "plan_limit",
            "This switcher has reached its plan's stream server limit. Upgrade the plan to add more.",
        )
    })?;
    state.notify_config(server.switcher_id).await;

    Ok((
        StatusCode::CREATED,
        Json(StreamServerDto::new(server, true)),
    ))
}

/// Change a stream server's name, address or whether it is used.
#[utoipa::path(patch, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers/{stream_server_id}", tag = "stream servers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("stream_server_id" = Uuid, Path)),
    request_body = UpdateStreamServer,
    responses((status = OK, body = StreamServerDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn update_stream_server(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<UpdateStreamServer>,
) -> ApiResult<Json<StreamServerDto>> {
    let (_, _, stream_server_id) = path;
    let access = authorized.into_access();
    let existing = server_in_switcher(&state, access.switcher.id, stream_server_id).await?;

    let name = request.name.as_deref().map(validate::name).transpose()?;
    let details = request
        .details
        .map(|input| {
            server_details::build(existing.kind, input, &existing.details(), &state.data_key)
        })
        .transpose()?;
    let stats_url = request
        .stats_url
        .as_deref()
        .map(|url| validate_stats_url(existing.kind, url, state.allow_private_stats_urls))
        .transpose()?;
    // The stored password or API key would go to the new address.
    if access.workspace.is_support()
        && stats_url.is_some_and(|url| url != existing.stats_url)
        && has_stored_secrets(&existing.details())
    {
        access.workspace.secrets_allowed()?;
    }

    let server = db::stream_servers::update(
        &state.db,
        stream_server_id,
        db::stream_servers::UpdateStreamServer {
            name,
            priority: None,
            enabled: request.enabled,
            stats_url,
            details: details.map(|details| details.to_value()),
            override_scene_live: None,
            override_scene_low: None,
            override_scene_offline: None,
        },
    )
    .await
    .map_err(name_conflict)?;
    state.notify_config(server.switcher_id).await;

    Ok(Json(StreamServerDto::new(server, true)))
}

/// Remove a stream server.
#[utoipa::path(delete, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers/{stream_server_id}", tag = "stream servers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("stream_server_id" = Uuid, Path)),
    responses((status = NO_CONTENT, description = "Removed"), (status = NOT_FOUND, body = ErrorBody)))]
async fn delete_stream_server(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let (_, _, stream_server_id) = path;
    let switcher_id = authorized.into_access().switcher.id;

    if db::stream_servers::delete(&state.db, switcher_id, stream_server_id).await? {
        state.notify_config(switcher_id).await;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("Stream server not found"))
    }
}

/// Replace a stream server's scene overrides.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers/{stream_server_id}/overrides", tag = "stream servers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("stream_server_id" = Uuid, Path)),
    request_body = OverridesDto,
    responses((status = OK, body = StreamServerDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn set_overrides(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<OverridesDto>,
) -> ApiResult<Json<StreamServerDto>> {
    let (_, _, stream_server_id) = path;
    let access = authorized.into_access();
    server_in_switcher(&state, access.switcher.id, stream_server_id).await?;

    let server = db::stream_servers::set_overrides(
        &state.db,
        stream_server_id,
        request.live,
        request.low,
        request.offline,
    )
    .await?;
    state.notify_config(server.switcher_id).await;

    Ok(Json(StreamServerDto::new(server, true)))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct TestStreamServer {
    pub kind: StreamServerKindDto,
    pub stats_url: String,
    /// The settings as typed. Secrets left out come from `stream_server_id`.
    #[serde(default)]
    pub details: DetailsInput,
    /// The saved server being edited, for its stored secrets.
    pub stream_server_id: Option<Uuid>,
}

/// Fetch a stats URL once and report what came back, before or after saving it.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers/test", tag = "stream servers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = TestStreamServer,
    responses(
        (status = OK, body = StatsProbe),
        (status = BAD_REQUEST, description = "A missing or invalid kind-specific field", body = ErrorBody),
    ))]
async fn test_stream_server(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<TestStreamServer>,
) -> ApiResult<Json<StatsProbe>> {
    let access = authorized.into_access();
    state.limits.outbound(access.workspace.user.id())?;
    if request.stats_url.len() > MAX_URL_LEN {
        return Err(ApiError::bad_request(
            "invalid_url",
            "The stats URL is too long",
        ));
    }
    let stored = match request.stream_server_id {
        Some(id) => {
            let server = server_in_switcher(&state, access.switcher.id, id).await?;
            let details = server.details();
            // Support may test a saved server, but not send its password or
            // API key somewhere else.
            if access.workspace.is_support()
                && request.stats_url.trim() != server.stats_url
                && has_stored_secrets(&details)
            {
                access.workspace.secrets_allowed()?;
            }
            details
        }
        None => Details::default(),
    };
    let details = server_details::build(
        request.kind.into(),
        request.details,
        &stored,
        &state.data_key,
    )?;
    let secrets = server_details::decrypt(&details, &state.data_key);
    let probe_request =
        stats_probe::request_for(request.kind, request.stats_url.trim(), &details, &secrets);

    Ok(Json(
        stats_probe::probe(
            request.kind,
            probe_request,
            &details,
            state.allow_private_stats_urls,
        )
        .await,
    ))
}

/// Set the order stream servers are tried in.
#[utoipa::path(put, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers/order", tag = "stream servers",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = ReorderStreamServers,
    responses(
        (status = OK, body = Vec<StreamServerDto>),
        (status = BAD_REQUEST, description = "`invalid_order` unless `ids` is every server once", body = ErrorBody),
    ))]
async fn reorder_stream_servers(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Json(request): Json<ReorderStreamServers>,
) -> ApiResult<Json<Vec<StreamServerDto>>> {
    let access = authorized.into_access();
    let switcher_id = access.switcher.id;

    if !db::stream_servers::reorder(&state.db, switcher_id, &request.ids).await? {
        return Err(ApiError::bad_request(
            "invalid_order",
            "The order must list every stream server of this switcher exactly once",
        ));
    }
    state.notify_config(switcher_id).await;
    let servers = db::stream_servers::by_switcher_id(&state.db, switcher_id).await?;

    Ok(Json(
        servers
            .into_iter()
            .map(|server| StreamServerDto::new(server, true))
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use db::stream_servers::StreamServerKind;

    use super::validate_stats_url;

    #[test]
    fn stats_urls_must_be_http_with_a_host() {
        let validate = |url| validate_stats_url(StreamServerKind::Mediamtx, url, false);
        assert!(validate("https://stats.srt.belabox.net/abc123").is_ok());
        assert_eq!(
            validate("  http://vps.example.net:9997/v3/paths/get/live  ").unwrap(),
            "http://vps.example.net:9997/v3/paths/get/live"
        );
        assert!(validate("ftp://example.net").is_err());
        assert!(validate("wss://example.net/ws").is_err());
        assert!(validate("example.net/stats").is_err());
        assert!(validate("").is_err());
    }

    #[test]
    fn private_ip_addresses_are_refused_unless_allowed() {
        let url = "http://169.254.169.254/latest/meta-data";
        assert!(validate_stats_url(StreamServerKind::Mediamtx, url, false).is_err());
        assert!(
            validate_stats_url(StreamServerKind::Mediamtx, "http://[::1]:9997/", false).is_err()
        );
        assert!(validate_stats_url(StreamServerKind::Mediamtx, url, true).is_ok());
    }

    #[test]
    fn websocket_stats_urls_must_be_ws() {
        let validate = |url| validate_stats_url(StreamServerKind::IrlhostingWebsocket, url, false);
        assert!(validate("wss://stats.example.net/ws?feed=feed1").is_ok());
        assert!(validate("ws://8.8.8.8:8080/ws").is_ok());
        assert!(validate("https://stats.example.net/ws").is_err());
    }
}
