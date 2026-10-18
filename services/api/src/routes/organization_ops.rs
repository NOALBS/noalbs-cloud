//! What an organization's team does across all its clients at once: the live
//! overview, the activity feed and the bulk actions (import clients, invite links,
//! allocations, delete). Single clients are in `organizations.rs`.

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Query, State},
};
use chrono::{DateTime, Duration, Utc};
use live::{EventDetail, FeedState, SwitcherEvent};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{OrgManage, OrgView, OrganizationAuthorized},
    routes::{
        ApiError, ApiResult, ErrorBody,
        organization_audit::record_team,
        organizations::{
            ClientDto, DEFAULT_EXPIRY_DAYS, MAX_EXPIRY_DAYS, allocation, ensure_client_room,
            ensure_not_suspended, limit_below_usage, live_statuses,
        },
    },
    validate,
};

/// Rows per page of the live overview.
const LIVE_PER_PAGE: usize = 25;
/// Activity events per request (default and most).
const DEFAULT_ACTIVITY: i64 = 50;
const MAX_ACTIVITY: i64 = 200;
/// The most clients one bulk action takes.
const BULK_MAX: usize = 100;
/// Tries at a free URL for an imported client before giving up.
const SLUG_TRIES: u32 = 30;

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(organization_live))
        .routes(routes!(organization_activity))
        .routes(routes!(bulk_create_clients))
        .routes(routes!(bulk_invite_customers))
        .routes(routes!(bulk_update_clients))
        .routes(routes!(bulk_delete_clients))
}

// --- Live overview ----------------------------------------------------------

/// What the organization's switchers are doing, in all.
#[derive(Debug, Default, Serialize, ToSchema)]
pub struct LiveCountsDto {
    pub clients: i64,
    pub switchers: i64,
    /// Switchers whose browser source is connected.
    pub connected: i64,
    pub obs_connected: i64,
    pub live: i64,
    /// Live and on the LOW scene.
    pub low: i64,
    /// Live and with no feed (the BRB scene).
    pub offline: i64,
    pub recording: i64,
    pub paused: i64,
}

/// A connected switcher of a client.
#[derive(Debug, Serialize, ToSchema)]
pub struct LiveSwitcherDto {
    pub switcher_id: Uuid,
    pub switcher_name: String,
    pub workspace_id: Uuid,
    pub workspace_name: String,
    pub workspace_slug: String,
    pub obs_connected: bool,
    pub streaming: bool,
    pub recording: bool,
    pub paused: bool,
    pub state: FeedState,
    pub stream_started_at: Option<DateTime<Utc>>,
    /// The scene on air.
    pub scene: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OrganizationLiveDto {
    pub counts: LiveCountsDto,
    /// Connected switchers on this page, live ones first.
    pub items: Vec<LiveSwitcherDto>,
    /// Connected switchers matching the filter, in all pages.
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

/// Which connected switchers the overview lists.
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LiveFilter {
    /// Streaming now.
    Live,
    /// OBS connected.
    Obs,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct LiveQuery {
    /// Part of a client's or switcher's name, or a client's URL.
    pub q: Option<String>,
    /// 1-based.
    pub page: Option<i64>,
    /// Every connected switcher when left out.
    #[param(inline)]
    pub filter: Option<LiveFilter>,
}

/// The organization's switchers that are connected now, live ones first, with
/// totals over all of them. Read from the status bucket, so it is current to a few
/// seconds.
#[utoipa::path(get, path = "/organizations/{organization_id}/live", tag = "organizations",
    params(("organization_id" = Uuid, Path), LiveQuery),
    responses((status = OK, body = OrganizationLiveDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn organization_live(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgView>,
    Query(query): Query<LiveQuery>,
) -> ApiResult<Json<OrganizationLiveDto>> {
    let access = authorized.into_access();
    let switchers =
        db::organizations::named_client_switchers(&state.db, access.organization_id).await?;
    let ids: Vec<Uuid> = switchers
        .iter()
        .map(|switcher| switcher.switcher_id)
        .collect();
    let statuses: HashMap<Uuid, live::SwitcherStatus> =
        live_statuses(&state, &ids).await.into_iter().collect();

    let mut counts = LiveCountsDto {
        clients: db::organizations::get_summary(
            &state.db,
            access.organization_id,
            access.user.id(),
        )
        .await?
        .map_or(0, |summary| summary.workspace_count),
        switchers: switchers.len() as i64,
        ..Default::default()
    };
    let mut connected: Vec<LiveSwitcherDto> = Vec::new();
    for switcher in switchers {
        let Some(status) = statuses.get(&switcher.switcher_id) else {
            continue;
        };
        let add = |yes: bool| i64::from(yes);
        counts.connected += 1;
        counts.obs_connected += add(status.obs_connected);
        counts.live += add(status.streaming);
        counts.recording += add(status.recording);
        counts.paused += add(status.paused);
        counts.low += add(status.streaming && status.state == FeedState::Low);
        counts.offline += add(status.streaming && status.state == FeedState::Offline);
        connected.push(LiveSwitcherDto {
            switcher_id: switcher.switcher_id,
            switcher_name: switcher.switcher_name,
            workspace_id: switcher.workspace_id,
            workspace_name: switcher.workspace_name,
            workspace_slug: switcher.workspace_slug,
            obs_connected: status.obs_connected,
            streaming: status.streaming,
            recording: status.recording,
            paused: status.paused,
            state: status.state,
            stream_started_at: status.stream_started_at,
            scene: status
                .current_scene
                .as_ref()
                .map(|scene| scene.name.clone()),
        });
    }

    let search = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(str::to_lowercase);
    connected.retain(|item| {
        let by_filter = match query.filter {
            Some(LiveFilter::Live) => item.streaming,
            Some(LiveFilter::Obs) => item.obs_connected,
            None => true,
        };
        let by_search = search.as_ref().is_none_or(|search| {
            [
                &item.workspace_name,
                &item.workspace_slug,
                &item.switcher_name,
            ]
            .iter()
            .any(|text| text.to_lowercase().contains(search))
        });
        by_filter && by_search
    });
    // Live first (longest-running first), then with OBS, then by name.
    connected.sort_by(|a, b| {
        (
            !a.streaming,
            a.stream_started_at,
            !a.obs_connected,
            &a.workspace_name,
        )
            .cmp(&(
                !b.streaming,
                b.stream_started_at,
                !b.obs_connected,
                &b.workspace_name,
            ))
    });
    let total = connected.len() as i64;
    let page = query.page.unwrap_or(1).max(1);
    let items = connected
        .into_iter()
        .skip((usize::try_from(page).unwrap_or(1) - 1) * LIVE_PER_PAGE)
        .take(LIVE_PER_PAGE)
        .collect();

    Ok(Json(OrganizationLiveDto {
        counts,
        items,
        total,
        page,
        per_page: LIVE_PER_PAGE as i64,
    }))
}

// --- Activity ---------------------------------------------------------------

/// An event of one of the organization's clients' switchers.
#[derive(Debug, Serialize, ToSchema)]
pub struct OrganizationEventDto {
    pub event: SwitcherEvent,
    pub switcher_id: Uuid,
    pub switcher_name: String,
    pub workspace_id: Uuid,
    pub workspace_name: String,
    pub workspace_slug: String,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct ActivityQuery {
    /// Only this client workspace.
    pub client: Option<Uuid>,
    /// Part of a client's or switcher's name, or a client's URL.
    pub q: Option<String>,
    /// Only these kinds of event, comma separated (e.g. `stream_started,stream_stopped`).
    pub kinds: Option<String>,
    /// Only events before this time, for paging back.
    pub before: Option<DateTime<Utc>>,
    /// At most this many events (default 50, up to 200).
    pub limit: Option<i64>,
}

/// The activity across all the organization's clients, newest first: what
/// happened on each switcher and who did it (the team, a customer, chat or the
/// automatic switcher). Admins only.
#[utoipa::path(get, path = "/organizations/{organization_id}/activity", tag = "organizations",
    params(("organization_id" = Uuid, Path), ActivityQuery),
    responses((status = OK, body = Vec<OrganizationEventDto>), (status = NOT_FOUND, body = ErrorBody)))]
async fn organization_activity(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgManage>,
    Query(query): Query<ActivityQuery>,
) -> ApiResult<Json<Vec<OrganizationEventDto>>> {
    let access = authorized.into_access();
    let limit = query
        .limit
        .unwrap_or(DEFAULT_ACTIVITY)
        .clamp(1, MAX_ACTIVITY);
    let kinds: Option<Vec<String>> = query.kinds.as_deref().map(|kinds| {
        kinds
            .split(',')
            .map(str::trim)
            .filter(|kind| {
                !kind.is_empty()
                    && kind.len() <= 40
                    && kind.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            })
            .take(40)
            .map(str::to_owned)
            .collect()
    });
    let filter = db::organizations::ActivityFilter {
        workspace_id: query.client,
        search: query.q.as_deref().map(str::trim).filter(|q| !q.is_empty()),
        kinds: kinds.as_deref(),
    };
    let rows = db::organizations::activity(
        &state.db,
        access.organization_id,
        filter,
        query.before,
        limit,
    )
    .await?;

    Ok(Json(
        rows.into_iter()
            .filter_map(|row| {
                Some(OrganizationEventDto {
                    event: SwitcherEvent {
                        id: row.id,
                        created_at: row.created_at,
                        detail: EventDetail::from_parts(&row.kind, row.data)?,
                    },
                    switcher_id: row.switcher_id,
                    switcher_name: row.switcher_name,
                    workspace_id: row.workspace_id,
                    workspace_name: row.workspace_name,
                    workspace_slug: row.workspace_slug,
                })
            })
            .collect(),
    ))
}

// --- Bulk actions -----------------------------------------------------------

/// Why one item of a bulk action didn't work.
#[derive(Debug, Serialize, ToSchema)]
pub struct BulkError {
    pub code: String,
    pub message: String,
}

impl From<ApiError> for BulkError {
    fn from(error: ApiError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.message().to_owned(),
        }
    }
}

impl From<db::Error> for BulkError {
    fn from(error: db::Error) -> Self {
        ApiError::from(error).into()
    }
}

fn too_many(count: usize) -> ApiResult<()> {
    if count == 0 || count > BULK_MAX {
        return Err(ApiError::bad_request(
            "invalid_bulk",
            format!("Pick between 1 and {BULK_MAX} clients"),
        ));
    }
    Ok(())
}

/// A URL from a name: lowercase letters and digits with single dashes, within the
/// URL length; `client` when nothing usable is left.
fn slug_base(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    let mut slug: String = slug.chars().take(validate::SLUG_MAX_LEN).collect();
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.len() < validate::SLUG_MIN_LEN {
        return "client".to_owned();
    }
    slug
}

/// The `attempt`th choice for a name's URL: the base, then `-2`, `-3`, ….
fn slug_candidate(base: &str, attempt: u32) -> String {
    if attempt == 0 {
        return base.to_owned();
    }
    let suffix = format!("-{}", attempt + 1);
    let mut head: String = base
        .chars()
        .take(validate::SLUG_MAX_LEN - suffix.len())
        .collect();
    while head.ends_with('-') {
        head.pop();
    }
    format!("{head}{suffix}")
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BulkCreateClients {
    /// One client per name (up to 100); the URL follows the name.
    pub names: Vec<String>,
    /// Most switchers each may have; `null` = only the pool limits it.
    pub switcher_allocation: Option<i32>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BulkCreateItem {
    pub name: String,
    pub client: Option<ClientDto>,
    pub error: Option<BulkError>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BulkCreateResult {
    pub items: Vec<BulkCreateItem>,
}

/// Create many client workspaces from a list of names. Each URL follows its name
/// (with a number added when it is taken); a name that fails doesn't stop the rest.
#[utoipa::path(post, path = "/organizations/{organization_id}/clients/bulk-create", tag = "organizations",
    params(("organization_id" = Uuid, Path)),
    request_body = BulkCreateClients,
    responses(
        (status = OK, body = BulkCreateResult),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = FORBIDDEN, description = "`organization_suspended`", body = ErrorBody),
    ))]
async fn bulk_create_clients(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgManage>,
    Json(request): Json<BulkCreateClients>,
) -> ApiResult<Json<BulkCreateResult>> {
    let access = authorized.into_access();
    too_many(request.names.len())?;
    let switcher_allocation = allocation(request.switcher_allocation)?;
    ensure_not_suspended(&state, access.organization_id).await?;
    ensure_client_room(&state, access.organization_id, request.names.len()).await?;

    let mut items = Vec::with_capacity(request.names.len());
    for raw in request.names {
        let outcome = create_one(
            &state,
            access.organization_id,
            access.user.id(),
            &raw,
            switcher_allocation,
        )
        .await;
        items.push(match outcome {
            Ok(client) => BulkCreateItem {
                name: client.name.clone(),
                client: Some(client),
                error: None,
            },
            Err(error) => BulkCreateItem {
                name: raw.trim().to_owned(),
                client: None,
                error: Some(error),
            },
        });
    }

    let created = items.iter().filter(|item| item.client.is_some()).count();
    record_team(
        &state,
        &access,
        "clients_imported",
        None,
        serde_json::json!({ "requested": items.len(), "created": created }),
    )
    .await?;

    Ok(Json(BulkCreateResult { items }))
}

async fn create_one(
    state: &ApiState,
    organization_id: Uuid,
    created_by: Uuid,
    raw_name: &str,
    switcher_allocation: Option<i32>,
) -> Result<ClientDto, BulkError> {
    let name = validate::name(raw_name)?;
    let base = slug_base(name);
    for attempt in 0..SLUG_TRIES {
        let slug = slug_candidate(&base, attempt);
        if validate::url_slug(&slug).is_err() {
            continue; // a reserved name
        }
        match db::organizations::create_client(
            &state.db,
            organization_id,
            created_by,
            name,
            &slug,
            switcher_allocation,
        )
        .await
        {
            Ok(workspace) => {
                let client =
                    db::organizations::get_client(&state.db, organization_id, workspace.id)
                        .await?
                        .ok_or_else(|| ApiError::not_found("Client not found"))?;
                return Ok(ClientDto::new(client, false, false));
            }
            Err(error) if error.constraint() == Some("workspaces_url_slug_key") => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err(ApiError::conflict("slug_taken", "No free URL was found for this name").into())
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BulkInvites {
    pub workspace_ids: Vec<Uuid>,
    /// 1 to 30 days; 7 when left out.
    pub expires_in_days: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BulkInviteItem {
    pub workspace_id: Uuid,
    pub name: String,
    pub url_slug: String,
    /// The invite's secret, shown only now; the dashboard turns it into a link.
    pub token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub error: Option<BulkError>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BulkInvitesResult {
    pub items: Vec<BulkInviteItem>,
}

/// Make a customer invite link (role admin) for each of the clients. Like the single
/// Invite button, for the whole team: whoever runs the clients hands them over.
#[utoipa::path(post, path = "/organizations/{organization_id}/clients/bulk-invites", tag = "organizations",
    params(("organization_id" = Uuid, Path)),
    request_body = BulkInvites,
    responses((status = OK, body = BulkInvitesResult), (status = BAD_REQUEST, body = ErrorBody)))]
async fn bulk_invite_customers(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgView>,
    Json(request): Json<BulkInvites>,
) -> ApiResult<Json<BulkInvitesResult>> {
    let access = authorized.into_access();
    too_many(request.workspace_ids.len())?;
    let days = request.expires_in_days.unwrap_or(DEFAULT_EXPIRY_DAYS);
    if !(1..=MAX_EXPIRY_DAYS).contains(&days) {
        return Err(ApiError::bad_request(
            "invalid_expiry",
            "Invites expire after 1 to 30 days",
        ));
    }

    let mut items = Vec::with_capacity(request.workspace_ids.len());
    for workspace_id in request.workspace_ids {
        let Some(client) =
            db::organizations::get_client(&state.db, access.organization_id, workspace_id).await?
        else {
            items.push(BulkInviteItem {
                workspace_id,
                name: String::new(),
                url_slug: String::new(),
                token: None,
                expires_at: None,
                error: Some(ApiError::not_found("Client not found").into()),
            });
            continue;
        };
        let made = async {
            super::plans::ensure_member_room(&state, workspace_id)
                .await
                .map_err(BulkError::from)?;
            let token = common::tokens::generate();
            let invite = db::invites::create(
                &state.db,
                workspace_id,
                db::organizations::CUSTOMER_ROLE,
                access.user.id(),
                &common::tokens::hash(&token),
                Utc::now() + Duration::days(days),
            )
            .await?;
            Ok::<_, BulkError>((token, invite.expires_at))
        }
        .await;
        let (token, expires_at, error) = match made {
            Ok((token, expires_at)) => (Some(token), Some(expires_at), None),
            Err(error) => (None, None, Some(error)),
        };
        items.push(BulkInviteItem {
            workspace_id,
            name: client.name,
            url_slug: client.url_slug,
            token,
            expires_at,
            error,
        });
    }

    let made = items.iter().filter(|item| item.token.is_some()).count();
    record_team(
        &state,
        &access,
        "client_invites_created",
        None,
        serde_json::json!({ "requested": items.len(), "made": made }),
    )
    .await?;

    Ok(Json(BulkInvitesResult { items }))
}

/// The outcome for one client of a bulk change.
#[derive(Debug, Serialize, ToSchema)]
pub struct BulkClientResult {
    pub workspace_id: Uuid,
    pub name: String,
    pub error: Option<BulkError>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BulkResult {
    pub items: Vec<BulkClientResult>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BulkAllocation {
    pub workspace_ids: Vec<Uuid>,
    /// Most switchers each may have; `null` = only the pool limits it. Can't go
    /// below a client's switchers (`limit_below_usage` for that one).
    pub switcher_allocation: Option<i32>,
}

/// Set the switcher allocation of many clients at once.
#[utoipa::path(put, path = "/organizations/{organization_id}/clients/bulk-allocation", tag = "organizations",
    params(("organization_id" = Uuid, Path)),
    request_body = BulkAllocation,
    responses((status = OK, body = BulkResult), (status = BAD_REQUEST, body = ErrorBody)))]
async fn bulk_update_clients(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgManage>,
    Json(request): Json<BulkAllocation>,
) -> ApiResult<Json<BulkResult>> {
    let access = authorized.into_access();
    too_many(request.workspace_ids.len())?;
    let switcher_allocation = allocation(request.switcher_allocation)?;

    let mut items = Vec::with_capacity(request.workspace_ids.len());
    for workspace_id in request.workspace_ids {
        let Some(client) =
            db::organizations::get_client(&state.db, access.organization_id, workspace_id).await?
        else {
            items.push(BulkClientResult {
                workspace_id,
                name: String::new(),
                error: Some(ApiError::not_found("Client not found").into()),
            });
            continue;
        };
        let error = match db::organizations::update_client(
            &state.db,
            access.organization_id,
            workspace_id,
            &client.name,
            switcher_allocation,
        )
        .await
        {
            Ok(db::organizations::LimitUpdate::Updated(_)) => None,
            Ok(db::organizations::LimitUpdate::NotFound) => {
                Some(ApiError::not_found("Client not found").into())
            }
            Ok(db::organizations::LimitUpdate::BelowUsage { used }) => {
                Some(limit_below_usage("The allocation", used).into())
            }
            Err(error) => Some(error.into()),
        };
        items.push(BulkClientResult {
            workspace_id,
            name: client.name,
            error,
        });
    }

    let changed = items.iter().filter(|item| item.error.is_none()).count();
    record_team(
        &state,
        &access,
        "clients_allocation_changed",
        None,
        serde_json::json!({
            "requested": items.len(),
            "changed": changed,
            "switcher_allocation": switcher_allocation,
        }),
    )
    .await?;

    Ok(Json(BulkResult { items }))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BulkDelete {
    pub workspace_ids: Vec<Uuid>,
}

/// Delete many client workspaces and everything in them.
#[utoipa::path(post, path = "/organizations/{organization_id}/clients/bulk-delete", tag = "organizations",
    params(("organization_id" = Uuid, Path)),
    request_body = BulkDelete,
    responses((status = OK, body = BulkResult), (status = BAD_REQUEST, body = ErrorBody)))]
async fn bulk_delete_clients(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgManage>,
    Json(request): Json<BulkDelete>,
) -> ApiResult<Json<BulkResult>> {
    let access = authorized.into_access();
    too_many(request.workspace_ids.len())?;

    let mut items = Vec::with_capacity(request.workspace_ids.len());
    for workspace_id in request.workspace_ids {
        let Some(client) =
            db::organizations::get_client(&state.db, access.organization_id, workspace_id).await?
        else {
            items.push(BulkClientResult {
                workspace_id,
                name: String::new(),
                error: Some(ApiError::not_found("Client not found").into()),
            });
            continue;
        };
        let error = crate::privacy::delete_workspace(&state, workspace_id, None)
            .await
            .err()
            .map(BulkError::from);
        if error.is_none() {
            tracing::info!(organization_id = %access.organization_id, %workspace_id, by = %access.user.id(), "Deleted a client workspace (bulk)");
        }
        items.push(BulkClientResult {
            workspace_id,
            name: client.name,
            error,
        });
    }

    let deleted: Vec<&str> = items
        .iter()
        .filter(|item| item.error.is_none())
        .map(|item| item.name.as_str())
        .collect();
    record_team(
        &state,
        &access,
        "clients_deleted",
        None,
        serde_json::json!({
            "requested": items.len(),
            "deleted": deleted.len(),
            // The first few, so the log says which.
            "names": deleted.iter().take(20).collect::<Vec<_>>(),
        }),
    )
    .await?;

    Ok(Json(BulkResult { items }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_follows_the_name() {
        assert_eq!(slug_base("Nomad Live"), "nomad-live");
        assert_eq!(slug_base("  --Kate's  IRL!! "), "kate-s-irl");
        assert_eq!(slug_base("Ab"), "client");
        assert_eq!(slug_base("日本語"), "client");
        assert_eq!(slug_base(&"a".repeat(80)).len(), validate::SLUG_MAX_LEN);
    }

    #[test]
    fn a_taken_url_gets_a_number() {
        assert_eq!(slug_candidate("nomad-live", 0), "nomad-live");
        assert_eq!(slug_candidate("nomad-live", 1), "nomad-live-2");
        assert_eq!(slug_candidate("nomad-live", 9), "nomad-live-10");
        let long = "a".repeat(validate::SLUG_MAX_LEN);
        let numbered = slug_candidate(&long, 1);
        assert_eq!(numbered.len(), validate::SLUG_MAX_LEN);
        assert!(numbered.ends_with("-2"));
        assert!(validate::url_slug(&numbered).is_ok());
    }

    #[test]
    fn a_reserved_name_is_skipped_for_a_numbered_one() {
        assert!(validate::url_slug(&slug_base("Admin")).is_err());
        assert!(validate::url_slug(&slug_candidate("admin", 1)).is_ok());
    }
}
