//! Markers: moments flagged on the running stream (the dashboard's button or
//! `!marker` in chat), listed on the stream's history page for copying.

use axum::{
    Json,
    extract::{Path, State},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{SwitcherAuthorized, SwitcherControl, SwitcherView},
    routes::{ApiResult, ErrorBody, feed::control_error, history::find_stream},
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(create_marker))
        .routes(routes!(list_markers))
}

/// A moment flagged during a stream.
#[derive(Debug, Serialize, ToSchema)]
pub struct MarkerDto {
    pub id: Uuid,
    pub at: DateTime<Utc>,
    /// Seconds into the stream.
    pub offset_seconds: i64,
    pub description: Option<String>,
    /// Who made it ("Name (Web)", "Name (Twitch chat)").
    pub created_by: Option<String>,
}

impl MarkerDto {
    fn new(marker: db::stream_markers::Marker, offset_seconds: i64) -> Self {
        Self {
            id: marker.id,
            at: marker.at,
            offset_seconds,
            description: marker.description,
            created_by: marker.created_by,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateMarker {
    /// What happened; up to 200 characters. Blank = no description.
    pub description: Option<String>,
}

/// Add a marker to the running stream.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/markers", tag = "history",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path)),
    request_body = CreateMarker,
    responses(
        (status = OK, description = "Added", body = MarkerDto),
        (status = BAD_REQUEST, description = "`invalid_description`", body = ErrorBody),
        (status = CONFLICT, description = "`not_streaming`", body = ErrorBody),
    ))]
async fn create_marker(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherControl>,
    Json(request): Json<CreateMarker>,
) -> ApiResult<Json<MarkerDto>> {
    let access = authorized.into_access();
    let by = access.workspace.display_name(&state).await?;
    let created = state
        .control()
        .create_marker(access.switcher.id, request.description.as_deref(), &by)
        .await
        .map_err(control_error)?;

    Ok(Json(MarkerDto::new(created.marker, created.offset_seconds)))
}

/// A stream's markers, in order.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/streams/{stream_id}/markers", tag = "history",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("stream_id" = Uuid, Path)),
    responses((status = OK, body = Vec<MarkerDto>), (status = NOT_FOUND, body = ErrorBody)))]
async fn list_markers(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
    Path(path): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<Json<Vec<MarkerDto>>> {
    let access = authorized.into_access();
    let (_, _, stream_id) = path;
    let stream = find_stream(&state, &access, stream_id).await?;
    let started_at = stream.stream.started_at;
    let markers = db::stream_markers::list(&state.db, stream.stream.id).await?;

    Ok(Json(
        markers
            .into_iter()
            .map(|marker| {
                let offset = (marker.at - started_at).num_seconds().max(0);
                MarkerDto::new(marker, offset)
            })
            .collect(),
    ))
}
