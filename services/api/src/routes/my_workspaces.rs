//! For a person who is a member of a lot of workspaces (an admin or operator for many
//! streamers): all their workspaces searched and paged with what each is doing, and a
//! live overview of all their switchers in one list. Only workspaces they are a member
//! of; an organization's team has its own pages (`organizations.rs`).

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Query, State},
};
use chrono::{DateTime, Utc};
use db::workspaces::MemberFilter;
use live::FeedState;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::CurrentUser,
    routes::{
        ApiResult, ErrorBody,
        organization_ops::{LiveFilter, LiveQuery},
        organizations::{ClientSort, ClientState, live_statuses},
        workspaces::{WorkspaceDto, WorkspaceRole},
    },
};

/// Workspaces per page.
const PER_PAGE: i64 = 25;
/// Rows per page of the live overview.
const LIVE_PER_PAGE: usize = 25;

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(my_workspaces))
        .routes(routes!(my_live))
}

/// One of the user's workspaces and what it is doing now.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyWorkspaceDto {
    pub workspace: WorkspaceDto,
    /// Some switcher has its OBS connected.
    pub obs_connected: bool,
    /// Some switcher is live.
    pub streaming: bool,
}

impl MyWorkspaceDto {
    /// What the dot in the list shows (the same words as a client's).
    fn state(&self) -> ClientState {
        if self.workspace.suspension.is_some() {
            ClientState::Suspended
        } else if self.streaming {
            ClientState::Live
        } else if self.obs_connected {
            ClientState::Online
        } else {
            ClientState::Offline
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MyWorkspacePage {
    pub items: Vec<MyWorkspaceDto>,
    /// Workspaces matching the search and filters, in all pages.
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct MyWorkspacesQuery {
    /// Part of the name or URL.
    pub q: Option<String>,
    /// 1-based.
    pub page: Option<i64>,
    /// Only where you have this role.
    #[param(inline)]
    pub role: Option<WorkspaceRole>,
    /// What the workspace is doing now; reads the live state of every matching workspace.
    #[param(inline)]
    pub state: Option<ClientState>,
    /// Oldest first when left out.
    #[param(inline)]
    pub sort: Option<ClientSort>,
}

/// Per workspace: whether some switcher has OBS connected, and whether some is live.
async fn workspace_live(
    state: &ApiState,
    user_id: Uuid,
    workspace_ids: &[Uuid],
) -> ApiResult<HashMap<Uuid, (bool, bool)>> {
    let switchers =
        db::workspaces::member_switchers(&state.db, user_id, Some(workspace_ids)).await?;
    let owner: HashMap<Uuid, Uuid> = switchers
        .iter()
        .map(|switcher| (switcher.switcher_id, switcher.workspace_id))
        .collect();
    let ids: Vec<Uuid> = owner.keys().copied().collect();

    let mut live: HashMap<Uuid, (bool, bool)> = HashMap::new();
    for (switcher_id, status) in live_statuses(state, &ids).await {
        let entry = live.entry(owner[&switcher_id]).or_default();
        entry.0 |= status.obs_connected;
        entry.1 |= status.streaming;
    }
    Ok(live)
}

/// The workspaces you are a member of, a page at a time, with whether they are live.
/// Filtering by what they are doing reads every matching workspace's switchers.
#[utoipa::path(get, path = "/me/workspaces", tag = "workspaces", params(MyWorkspacesQuery),
    responses((status = OK, body = MyWorkspacePage), (status = UNAUTHORIZED, body = ErrorBody)))]
async fn my_workspaces(
    State(state): State<ApiState>,
    user: CurrentUser,
    Query(query): Query<MyWorkspacesQuery>,
) -> ApiResult<Json<MyWorkspacePage>> {
    let page = query.page.unwrap_or(1).max(1);
    let filter = MemberFilter {
        search: query.q.as_deref().map(str::trim).filter(|q| !q.is_empty()),
        role: query.role.map(Into::into),
        by_name: matches!(query.sort, Some(ClientSort::Name)),
    };
    let (limit, offset) = match query.state {
        Some(_) => (db::workspaces::MAX_MEMBER_SWITCHERS, 0),
        None => (PER_PAGE, (page - 1) * PER_PAGE),
    };
    let (summaries, total) =
        db::workspaces::list_summaries_page(&state.db, user.id(), filter, limit, offset).await?;
    let ids: Vec<Uuid> = summaries.iter().map(|workspace| workspace.id).collect();
    let live = workspace_live(&state, user.id(), &ids).await?;
    let mut items: Vec<MyWorkspaceDto> = summaries
        .into_iter()
        .map(|summary| {
            let (obs_connected, streaming) = live.get(&summary.id).copied().unwrap_or_default();
            MyWorkspaceDto {
                workspace: summary.into(),
                obs_connected,
                streaming,
            }
        })
        .collect();

    let total = match query.state {
        Some(wanted) => {
            items.retain(|item| item.state() == wanted);
            let total = items.len() as i64;
            items = items
                .into_iter()
                .skip(usize::try_from((page - 1) * PER_PAGE).unwrap_or(usize::MAX))
                .take(PER_PAGE as usize)
                .collect();
            total
        }
        None => total,
    };

    Ok(Json(MyWorkspacePage {
        items,
        total,
        page,
        per_page: PER_PAGE,
    }))
}

/// What all the user's switchers are doing, in all.
#[derive(Debug, Default, Serialize, ToSchema)]
pub struct MyLiveCountsDto {
    pub workspaces: i64,
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

/// A connected switcher of one of the user's workspaces.
#[derive(Debug, Serialize, ToSchema)]
pub struct MyLiveSwitcherDto {
    pub switcher_id: Uuid,
    pub switcher_name: String,
    pub workspace_id: Uuid,
    pub workspace_name: String,
    pub workspace_slug: String,
    /// Your role in the workspace.
    pub role: WorkspaceRole,
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
pub struct MyLiveDto {
    pub counts: MyLiveCountsDto,
    /// Connected switchers on this page, live ones first.
    pub items: Vec<MyLiveSwitcherDto>,
    /// Connected switchers matching the filter, in all pages.
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

/// Your switchers that are connected now, from every workspace you are a member of, live
/// ones first, with totals over all of them. Read from the status bucket, so it is current
/// to a few seconds.
#[utoipa::path(get, path = "/me/live", tag = "workspaces", params(LiveQuery),
    responses((status = OK, body = MyLiveDto), (status = UNAUTHORIZED, body = ErrorBody)))]
async fn my_live(
    State(state): State<ApiState>,
    user: CurrentUser,
    Query(query): Query<LiveQuery>,
) -> ApiResult<Json<MyLiveDto>> {
    let switchers = db::workspaces::member_switchers(&state.db, user.id(), None).await?;
    let ids: Vec<Uuid> = switchers
        .iter()
        .map(|switcher| switcher.switcher_id)
        .collect();
    let statuses: HashMap<Uuid, live::SwitcherStatus> =
        live_statuses(&state, &ids).await.into_iter().collect();
    let (_, workspaces) =
        db::workspaces::list_summaries_page(&state.db, user.id(), MemberFilter::default(), 1, 0)
            .await?;

    let mut counts = MyLiveCountsDto {
        workspaces,
        switchers: switchers.len() as i64,
        ..Default::default()
    };
    let mut connected: Vec<MyLiveSwitcherDto> = Vec::new();
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
        connected.push(MyLiveSwitcherDto {
            switcher_id: switcher.switcher_id,
            switcher_name: switcher.switcher_name,
            workspace_id: switcher.workspace_id,
            workspace_name: switcher.workspace_name,
            workspace_slug: switcher.workspace_slug,
            role: switcher.role.into(),
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

    Ok(Json(MyLiveDto {
        counts,
        items,
        total,
        page,
        per_page: LIVE_PER_PAGE as i64,
    }))
}
