//! Stream history: past (and running) streams with their stats, kept for the
//! workspace plan's `history_days`. obs-link records it (`db::streams`); an
//! upkeep task here closes abandoned streams and deletes expired ones.

use std::time::Duration;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{DateTime, TimeDelta, Utc};
use common::shutdown::ShutdownHandler;
use db::streams::BUCKET_SECONDS;
use live::{EventDetail, SwitcherEvent};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{SwitcherAccess, SwitcherAuthorized, SwitcherManage, SwitcherView},
    routes::{ApiError, ApiResult, ErrorBody, stream_servers::StreamServerKindDto},
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_streams))
        .routes(routes!(get_stream, delete_stream))
        .routes(routes!(stream_raw))
}

const DEFAULT_STREAMS: i64 = 20;
const MAX_STREAMS: i64 = 100;
/// Points per server a chart gets; a 10 h stream comes back in 30 s steps.
const MAX_POINTS: i64 = 1200;
/// Raw snapshots are bigger, so fewer of them.
const MAX_RAW_POINTS: i64 = 600;
const MAX_EVENTS: i64 = 1000;

/// How often the upkeep runs.
const UPKEEP_EVERY: Duration = Duration::from_secs(10 * 60);
/// An open stream with no samples for this long lost its browser source for good.
const STALE_AFTER: TimeDelta = TimeDelta::minutes(30);
const DELETE_BATCH: i64 = 500;

/// A stream, from OBS starting to stopping it.
#[derive(Debug, Serialize, ToSchema)]
pub struct StreamSummaryDto {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    /// `null` while it's still going.
    pub ended_at: Option<DateTime<Utc>>,
    /// How long it ran (so far).
    pub duration_seconds: i64,
    /// Seconds the switcher saw the feed as LIVE / LOW / BRB.
    pub live_seconds: i32,
    pub low_seconds: i32,
    pub offline_seconds: i32,
    /// Automatic scene switches while it ran.
    pub auto_switches: i64,
}

impl From<db::streams::StreamSummary> for StreamSummaryDto {
    fn from(summary: db::streams::StreamSummary) -> Self {
        let stream = summary.stream;
        let end = stream.ended_at.unwrap_or_else(Utc::now);
        Self {
            id: stream.id,
            started_at: stream.started_at,
            ended_at: stream.ended_at,
            duration_seconds: (end - stream.started_at).num_seconds().max(0),
            live_seconds: stream.live_seconds,
            low_seconds: stream.low_seconds,
            offline_seconds: stream.offline_seconds,
            auto_switches: summary.auto_switches,
        }
    }
}

/// A stream server that fed the stream, named as it was then.
#[derive(Debug, Serialize, ToSchema)]
pub struct StreamSourceDto {
    pub server_id: Uuid,
    pub name: String,
    pub kind: StreamServerKindDto,
}

/// One server's numbers over one step.
#[derive(Debug, Serialize, ToSchema)]
pub struct SeriesPointDto {
    /// Start of the step.
    pub at: DateTime<Utc>,
    /// Average bitrate in kbps.
    pub bitrate: f64,
    pub bitrate_min: i32,
    pub bitrate_max: i32,
    /// Average RTT in ms, for servers that report it.
    pub rtt: Option<f64>,
    pub rtt_max: Option<f64>,
    /// Seconds this server's reading was LIVE / LOW / BRB.
    pub live_seconds: i64,
    pub low_seconds: i64,
    pub offline_seconds: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ServerSeriesDto {
    pub server_id: Uuid,
    pub points: Vec<SeriesPointDto>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StreamDetailDto {
    pub stream: StreamSummaryDto,
    pub sources: Vec<StreamSourceDto>,
    /// The range the series and events cover.
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// Length of each point's step.
    pub step_seconds: i64,
    pub series: Vec<ServerSeriesDto>,
    /// Activity while it ran, oldest first.
    pub events: Vec<SwitcherEvent>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RawPointDto {
    pub at: DateTime<Utc>,
    /// The stats as the server reported them.
    #[schema(value_type = Object)]
    pub raw: serde_json::Value,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RawSeriesDto {
    pub step_seconds: i64,
    pub points: Vec<RawPointDto>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct StreamsQuery {
    /// At most this many streams (default 20, up to 100).
    pub limit: Option<i64>,
    /// Only streams that started before this time, for paging back.
    pub before: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct RangeQuery {
    /// Start of the range to zoom into (default: the stream's start).
    pub from: Option<DateTime<Utc>>,
    /// End of the range (default: the stream's end, or now).
    pub to: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct RawQuery {
    pub server_id: Uuid,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

async fn history_days(state: &ApiState, access: &SwitcherAccess) -> ApiResult<i32> {
    let plan = db::plans::for_workspace(&state.db, access.workspace.workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Workspace not found"))?;
    Ok(plan.history_days)
}

pub(super) async fn find_stream(
    state: &ApiState,
    access: &SwitcherAccess,
    stream_id: Uuid,
) -> ApiResult<db::streams::StreamSummary> {
    let days = history_days(state, access).await?;
    db::streams::get(&state.db, access.switcher.id, days, stream_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Stream not found"))
}

/// `at`, rounded down to a bucket boundary.
fn floor_to_bucket(at: DateTime<Utc>) -> DateTime<Utc> {
    let seconds = at.timestamp();
    DateTime::from_timestamp(seconds - seconds.rem_euclid(BUCKET_SECONDS), 0).unwrap_or(at)
}

/// The step that fits `[from, to)` into at most `max_points`, in whole buckets.
fn step_seconds(from: DateTime<Utc>, to: DateTime<Utc>, max_points: i64) -> i64 {
    let span = (to - from).num_seconds().max(1);
    let step = (span + max_points - 1) / max_points;
    let buckets = (step + BUCKET_SECONDS - 1) / BUCKET_SECONDS;
    buckets.max(1) * BUCKET_SECONDS
}

/// The part of the stream to show: `from`/`to` clamped to it, on bucket boundaries.
fn range(
    stream: &db::streams::Stream,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
) -> ApiResult<(DateTime<Utc>, DateTime<Utc>)> {
    let start = floor_to_bucket(stream.started_at);
    // Just past the last sample, so its bucket is in.
    let last = stream.last_sample_at.unwrap_or(stream.started_at) + TimeDelta::seconds(1);
    let end = stream.ended_at.unwrap_or_else(Utc::now).max(last);

    let from = floor_to_bucket(from.unwrap_or(start).max(start));
    let to = to.unwrap_or(end).min(end);
    if from >= to {
        return Err(ApiError::bad_request(
            "invalid_range",
            "The range is empty or outside the stream.",
        ));
    }
    Ok((from, to))
}

/// The switcher's streams, newest first (only those the plan still keeps).
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/streams", tag = "history",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), StreamsQuery),
    responses((status = OK, body = Vec<StreamSummaryDto>), (status = FORBIDDEN, body = ErrorBody)))]
async fn list_streams(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
    Query(query): Query<StreamsQuery>,
) -> ApiResult<Json<Vec<StreamSummaryDto>>> {
    let access = authorized.into_access();
    let days = history_days(&state, &access).await?;
    let limit = query.limit.unwrap_or(DEFAULT_STREAMS).clamp(1, MAX_STREAMS);
    let streams =
        db::streams::list(&state.db, access.switcher.id, days, query.before, limit).await?;

    Ok(Json(streams.into_iter().map(Into::into).collect()))
}

/// One stream: its stats per server over `from`..`to` and its activity.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/streams/{stream_id}", tag = "history",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("stream_id" = Uuid, Path), RangeQuery),
    responses((status = OK, body = StreamDetailDto), (status = BAD_REQUEST, body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody)))]
async fn get_stream(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherView>,
    Path(path): Path<(Uuid, Uuid, Uuid)>,
    Query(query): Query<RangeQuery>,
) -> ApiResult<Json<StreamDetailDto>> {
    let (_, _, stream_id) = path;
    let access = authorized.into_access();
    let summary = find_stream(&state, &access, stream_id).await?;
    let (from, to) = range(&summary.stream, query.from, query.to)?;
    let step = step_seconds(from, to, MAX_POINTS);

    let sources = db::streams::sources(&state.db, stream_id).await?;
    let points = db::streams::series(&state.db, stream_id, from, to, step).await?;
    let events = db::switcher_events::between(
        &state.db,
        access.switcher.id,
        summary.stream.started_at.max(from),
        to,
        MAX_EVENTS,
    )
    .await?;

    let mut series: Vec<ServerSeriesDto> = Vec::new();
    for point in points {
        if series
            .last()
            .is_none_or(|last| last.server_id != point.server_id)
        {
            series.push(ServerSeriesDto {
                server_id: point.server_id,
                points: Vec::new(),
            });
        }
        if let Some(last) = series.last_mut() {
            last.points.push(SeriesPointDto {
                at: point.at,
                bitrate: point.bitrate_avg.unwrap_or(0.0),
                bitrate_min: point.bitrate_min,
                bitrate_max: point.bitrate_max,
                rtt: point.rtt_avg,
                rtt_max: point.rtt_max,
                live_seconds: point.live_seconds,
                low_seconds: point.low_seconds,
                offline_seconds: point.offline_seconds,
            });
        }
    }

    Ok(Json(StreamDetailDto {
        stream: summary.into(),
        sources: sources
            .into_iter()
            .map(|source| StreamSourceDto {
                server_id: source.server_id,
                name: source.name,
                kind: source.kind.into(),
            })
            .collect(),
        from,
        to,
        step_seconds: step,
        series,
        events: events
            .into_iter()
            .filter_map(|row| {
                Some(SwitcherEvent {
                    id: row.id,
                    created_at: row.created_at,
                    detail: EventDetail::from_parts(&row.kind, row.data)?,
                })
            })
            .collect(),
    }))
}

/// Delete a stream from the history.
#[utoipa::path(delete, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/streams/{stream_id}", tag = "history",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("stream_id" = Uuid, Path)),
    responses((status = NO_CONTENT, description = "Deleted"), (status = NOT_FOUND, body = ErrorBody)))]
async fn delete_stream(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let (_, _, stream_id) = path;
    let switcher_id = authorized.into_access().switcher.id;
    if db::streams::delete(&state.db, switcher_id, stream_id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("Stream not found"))
    }
}

/// One server's raw stats during a stream, as the server reported them. Only
/// for managers: raw stats can hold stream names or the publisher's address
/// (not NOALBS support).
#[utoipa::path(get, path = "/workspaces/{workspace_id}/switchers/{switcher_id}/streams/{stream_id}/raw", tag = "history",
    params(("workspace_id" = Uuid, Path), ("switcher_id" = Uuid, Path), ("stream_id" = Uuid, Path), RawQuery),
    responses((status = OK, body = RawSeriesDto), (status = BAD_REQUEST, body = ErrorBody),
        (status = FORBIDDEN, description = "`support_secret` for NOALBS support", body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody)))]
async fn stream_raw(
    State(state): State<ApiState>,
    authorized: SwitcherAuthorized<SwitcherManage>,
    Path(path): Path<(Uuid, Uuid, Uuid)>,
    Query(query): Query<RawQuery>,
) -> ApiResult<Json<RawSeriesDto>> {
    let (_, _, stream_id) = path;
    let access = authorized.into_access();
    access.workspace.secrets_allowed()?;
    let summary = find_stream(&state, &access, stream_id).await?;
    let (from, to) = range(&summary.stream, query.from, query.to)?;
    let step = step_seconds(from, to, MAX_RAW_POINTS);
    let points =
        db::streams::raw_series(&state.db, stream_id, query.server_id, from, to, step).await?;

    Ok(Json(RawSeriesDto {
        step_seconds: step,
        points: points
            .into_iter()
            .map(|point| RawPointDto {
                at: point.at,
                raw: point.raw,
            })
            .collect(),
    }))
}

/// Closes streams whose browser source never came back and deletes history
/// past each plan's `history_days`. Safe to run on every API replica.
pub fn spawn_upkeep(db: db::DbPool, mut shutdown: ShutdownHandler) {
    tokio::spawn(async move {
        let mut every = tokio::time::interval(UPKEEP_EVERY);
        loop {
            tokio::select! {
                _ = shutdown.changed() => return,
                _ = every.tick() => upkeep(&db).await,
            }
        }
    });
}

async fn upkeep(db: &db::DbPool) {
    match db::streams::close_stale(db, Utc::now() - STALE_AFTER).await {
        Ok(0) => {}
        Ok(closed) => tracing::info!(closed, "Closed abandoned streams"),
        Err(error) => tracing::warn!(%error, "Couldn't close abandoned streams"),
    }

    let mut deleted = 0;
    loop {
        match db::streams::delete_expired(db, DELETE_BATCH).await {
            Ok(count) => {
                deleted += count;
                if count < DELETE_BATCH as u64 {
                    break;
                }
            }
            Err(error) => {
                tracing::warn!(%error, "Couldn't delete expired stream history");
                break;
            }
        }
    }
    if deleted > 0 {
        tracing::info!(deleted, "Deleted expired stream history");
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeDelta, Utc};

    use super::{floor_to_bucket, step_seconds};

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().to_utc()
    }

    #[test]
    fn steps_are_whole_buckets_that_fit_the_limit() {
        let from = at("2026-09-01T20:00:00Z");
        // Short streams show every bucket.
        assert_eq!(step_seconds(from, from + TimeDelta::minutes(5), 1200), 10);
        assert_eq!(step_seconds(from, from + TimeDelta::hours(3), 1200), 10);
        // Ten hours: 30 s steps, 1200 points.
        assert_eq!(step_seconds(from, from + TimeDelta::hours(10), 1200), 30);
        // Rounds up to the next bucket rather than overshooting the limit.
        assert_eq!(step_seconds(from, from + TimeDelta::hours(4), 1200), 20);
        assert_eq!(step_seconds(from, from, 1200), 10);
    }

    #[test]
    fn floors_to_ten_seconds() {
        assert_eq!(
            floor_to_bucket(at("2026-09-01T20:00:17.5Z")),
            at("2026-09-01T20:00:10Z")
        );
    }
}
