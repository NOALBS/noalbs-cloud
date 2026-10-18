//! Stream history: one row per OBS stream, the servers that fed it, and their
//! stats in 10-second buckets. obs-link writes it while streaming; the API reads
//! it and keeps it within the plan's `history_days`.

use chrono::{DateTime, TimeDelta, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{error::Result, stream_servers::StreamServerKind};

/// Stats are stored per server in buckets this long.
pub const BUCKET_SECONDS: i64 = 10;

/// A running stream whose start is this close to a stored one is the same
/// stream: obs-link reconnected, or the switcher restarted mid-stream.
pub const RESUME_TOLERANCE: TimeDelta = TimeDelta::minutes(2);

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Stream {
    pub id: Uuid,
    pub switcher_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub last_sample_at: Option<DateTime<Utc>>,
    pub live_seconds: i32,
    pub low_seconds: i32,
    pub offline_seconds: i32,
}

/// A [`Stream`] with what the history list shows about it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StreamSummary {
    #[sqlx(flatten)]
    pub stream: Stream,
    /// Automatic scene switches while it ran.
    pub auto_switches: i64,
}

/// A stream server that fed a stream, named as it was then.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Source {
    pub server_id: Uuid,
    pub name: String,
    pub kind: StreamServerKind,
}

/// One server's numbers over one bucket.
#[derive(Debug, Clone, PartialEq)]
pub struct Bucket {
    pub server_id: Uuid,
    pub bucket_at: DateTime<Utc>,
    pub samples: i32,
    pub bitrate_sum: i64,
    pub bitrate_min: i32,
    pub bitrate_max: i32,
    pub rtt_sum: f64,
    pub rtt_samples: i32,
    pub rtt_max: Option<f64>,
    pub live_seconds: i32,
    pub low_seconds: i32,
    pub offline_seconds: i32,
    pub raw: Option<serde_json::Value>,
}

/// Seconds the switcher spent in each feed state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StateSeconds {
    pub live: i32,
    pub low: i32,
    pub offline: i32,
}

/// One server's numbers over one step of a [`series`].
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SeriesPoint {
    pub server_id: Uuid,
    pub at: DateTime<Utc>,
    pub bitrate_avg: Option<f64>,
    pub bitrate_min: i32,
    pub bitrate_max: i32,
    pub rtt_avg: Option<f64>,
    pub rtt_max: Option<f64>,
    pub live_seconds: i64,
    pub low_seconds: i64,
    pub offline_seconds: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RawPoint {
    pub at: DateTime<Utc>,
    pub raw: serde_json::Value,
}

macro_rules! stream_columns {
    () => {
        "streams.id, streams.switcher_id, streams.started_at, streams.ended_at, \
         streams.last_sample_at, streams.live_seconds, streams.low_seconds, \
         streams.offline_seconds"
    };
}

/// Closes the switcher's open stream, if any, at its last sample.
async fn close_open<'a>(
    executor: impl sqlx::PgExecutor<'a>,
    switcher_id: Uuid,
    except: Option<Uuid>,
) -> Result<()> {
    sqlx::query(
        "
        UPDATE streams SET ended_at = COALESCE(last_sample_at, started_at)
        WHERE switcher_id = $1 AND ended_at IS NULL AND ($2::uuid IS NULL OR id <> $2)
        ",
    )
    .bind(switcher_id)
    .bind(except)
    .execute(executor)
    .await?;
    Ok(())
}

async fn insert<'a>(
    executor: impl sqlx::PgExecutor<'a>,
    switcher_id: Uuid,
    started_at: DateTime<Utc>,
) -> Result<Stream> {
    let stream = sqlx::query_as::<_, Stream>(concat!(
        "INSERT INTO streams (switcher_id, started_at) VALUES ($1, $2) RETURNING ",
        stream_columns!()
    ))
    .bind(switcher_id)
    .bind(started_at)
    .fetch_one(executor)
    .await?;
    Ok(stream)
}

/// A new stream that started at `started_at`, closing any still open.
#[tracing::instrument(skip(pool))]
pub async fn start(pool: &PgPool, switcher_id: Uuid, started_at: DateTime<Utc>) -> Result<Stream> {
    let mut tx = pool.begin().await?;
    close_open(&mut *tx, switcher_id, None).await?;
    let stream = insert(&mut *tx, switcher_id, started_at).await?;
    tx.commit().await?;
    Ok(stream)
}

/// The stream OBS is already running, which started at about `started_at`: the
/// stored one (reopened if the upkeep closed it meanwhile), or a new one.
#[tracing::instrument(skip(pool))]
pub async fn resume(pool: &PgPool, switcher_id: Uuid, started_at: DateTime<Utc>) -> Result<Stream> {
    let mut tx = pool.begin().await?;
    let existing = sqlx::query_scalar::<_, Uuid>(
        "
        SELECT id FROM streams
        WHERE switcher_id = $1 AND started_at BETWEEN $2 AND $3
        ORDER BY started_at DESC
        LIMIT 1
        FOR UPDATE
        ",
    )
    .bind(switcher_id)
    .bind(started_at - RESUME_TOLERANCE)
    .bind(started_at + RESUME_TOLERANCE)
    .fetch_optional(&mut *tx)
    .await?;

    let stream = match existing {
        Some(id) => {
            close_open(&mut *tx, switcher_id, Some(id)).await?;
            sqlx::query_as::<_, Stream>(concat!(
                "UPDATE streams SET ended_at = NULL WHERE id = $1 RETURNING ",
                stream_columns!()
            ))
            .bind(id)
            .fetch_one(&mut *tx)
            .await?
        }
        None => {
            close_open(&mut *tx, switcher_id, None).await?;
            insert(&mut *tx, switcher_id, started_at).await?
        }
    };
    tx.commit().await?;
    Ok(stream)
}

/// The switcher's running stream, if any (there is at most one).
#[tracing::instrument(skip(pool))]
pub async fn open(pool: &PgPool, switcher_id: Uuid) -> Result<Option<Stream>> {
    let stream = sqlx::query_as::<_, Stream>(concat!(
        "SELECT ",
        stream_columns!(),
        " FROM streams WHERE switcher_id = $1 AND ended_at IS NULL"
    ))
    .bind(switcher_id)
    .fetch_optional(pool)
    .await?;
    Ok(stream)
}

/// Ends the switcher's open stream at `at`, or at its last sample.
#[tracing::instrument(skip(pool))]
pub async fn end_open(pool: &PgPool, switcher_id: Uuid, at: Option<DateTime<Utc>>) -> Result<()> {
    sqlx::query(
        "
        UPDATE streams
        SET ended_at = GREATEST(COALESCE($2, last_sample_at, started_at), started_at)
        WHERE switcher_id = $1 AND ended_at IS NULL
        ",
    )
    .bind(switcher_id)
    .bind(at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Records the servers feeding a stream, updating names that changed.
#[tracing::instrument(skip(pool, sources))]
pub async fn upsert_sources(pool: &PgPool, stream_id: Uuid, sources: &[Source]) -> Result<()> {
    let mut tx = pool.begin().await?;
    for source in sources {
        sqlx::query(
            "
            INSERT INTO stream_sources (stream_id, server_id, name, kind)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (stream_id, server_id) DO UPDATE SET name = EXCLUDED.name
            ",
        )
        .bind(stream_id)
        .bind(source.server_id)
        .bind(&source.name)
        .bind(source.kind)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Adds buckets to a stream (merging into ones already written) and counts the
/// switcher's state seconds.
#[tracing::instrument(skip(pool, buckets))]
pub async fn write_buckets(
    pool: &PgPool,
    stream_id: Uuid,
    buckets: &[Bucket],
    states: StateSeconds,
    last_sample_at: DateTime<Utc>,
) -> Result<()> {
    let mut tx = pool.begin().await?;

    if !buckets.is_empty() {
        let column = |f: fn(&Bucket) -> i32| buckets.iter().map(f).collect::<Vec<_>>();
        sqlx::query(
            "
            INSERT INTO stream_stats (
                stream_id, server_id, bucket_at, samples, bitrate_sum, bitrate_min, bitrate_max,
                rtt_sum, rtt_samples, rtt_max, live_seconds, low_seconds, offline_seconds, raw
            )
            SELECT $1, * FROM UNNEST(
                $2::uuid[], $3::timestamptz[], $4::int4[], $5::int8[], $6::int4[], $7::int4[],
                $8::float8[], $9::int4[], $10::float8[], $11::int4[], $12::int4[], $13::int4[],
                $14::jsonb[]
            )
            ON CONFLICT (stream_id, server_id, bucket_at) DO UPDATE SET
                samples = stream_stats.samples + EXCLUDED.samples,
                bitrate_sum = stream_stats.bitrate_sum + EXCLUDED.bitrate_sum,
                bitrate_min = LEAST(stream_stats.bitrate_min, EXCLUDED.bitrate_min),
                bitrate_max = GREATEST(stream_stats.bitrate_max, EXCLUDED.bitrate_max),
                rtt_sum = stream_stats.rtt_sum + EXCLUDED.rtt_sum,
                rtt_samples = stream_stats.rtt_samples + EXCLUDED.rtt_samples,
                rtt_max = GREATEST(stream_stats.rtt_max, EXCLUDED.rtt_max),
                live_seconds = stream_stats.live_seconds + EXCLUDED.live_seconds,
                low_seconds = stream_stats.low_seconds + EXCLUDED.low_seconds,
                offline_seconds = stream_stats.offline_seconds + EXCLUDED.offline_seconds,
                raw = COALESCE(EXCLUDED.raw, stream_stats.raw)
            ",
        )
        .bind(stream_id)
        .bind(buckets.iter().map(|b| b.server_id).collect::<Vec<_>>())
        .bind(buckets.iter().map(|b| b.bucket_at).collect::<Vec<_>>())
        .bind(column(|b| b.samples))
        .bind(buckets.iter().map(|b| b.bitrate_sum).collect::<Vec<_>>())
        .bind(column(|b| b.bitrate_min))
        .bind(column(|b| b.bitrate_max))
        .bind(buckets.iter().map(|b| b.rtt_sum).collect::<Vec<_>>())
        .bind(column(|b| b.rtt_samples))
        .bind(buckets.iter().map(|b| b.rtt_max).collect::<Vec<_>>())
        .bind(column(|b| b.live_seconds))
        .bind(column(|b| b.low_seconds))
        .bind(column(|b| b.offline_seconds))
        .bind(buckets.iter().map(|b| b.raw.clone()).collect::<Vec<_>>())
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "
        UPDATE streams SET
            live_seconds = live_seconds + $2,
            low_seconds = low_seconds + $3,
            offline_seconds = offline_seconds + $4,
            last_sample_at = GREATEST(last_sample_at, $5)
        WHERE id = $1
        ",
    )
    .bind(stream_id)
    .bind(states.live)
    .bind(states.low)
    .bind(states.offline)
    .bind(last_sample_at)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

/// Selects [`StreamSummary`]s of switcher `$1` that are still within the plan's
/// retention of `$2` days: kept that long after they end (open ones always count).
macro_rules! summaries_within_retention {
    () => {
        concat!(
            "SELECT ",
            stream_columns!(),
            ",
            (SELECT count(*) FROM switcher_events
             WHERE switcher_events.switcher_id = streams.switcher_id
               AND switcher_events.kind = 'scene_switched'
               AND switcher_events.data->>'reason' = 'auto'
               AND switcher_events.created_at >= streams.started_at
               AND switcher_events.created_at <= COALESCE(streams.ended_at, now())
            ) AS auto_switches
            FROM streams
            WHERE streams.switcher_id = $1
              AND COALESCE(streams.ended_at, now()) >= now() - make_interval(days => $2)"
        )
    };
}

/// The switcher's streams within its plan's retention, newest first; `before`
/// pages further back.
#[tracing::instrument(skip(pool))]
pub async fn list(
    pool: &PgPool,
    switcher_id: Uuid,
    history_days: i32,
    before: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<Vec<StreamSummary>> {
    let streams = sqlx::query_as::<_, StreamSummary>(concat!(
        summaries_within_retention!(),
        "
          AND ($3::timestamptz IS NULL OR streams.started_at < $3)
        ORDER BY streams.started_at DESC
        LIMIT $4
        "
    ))
    .bind(switcher_id)
    .bind(history_days)
    .bind(before)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(streams)
}

/// One of the switcher's streams, if it's still within the plan's retention.
#[tracing::instrument(skip(pool))]
pub async fn get(
    pool: &PgPool,
    switcher_id: Uuid,
    history_days: i32,
    stream_id: Uuid,
) -> Result<Option<StreamSummary>> {
    let stream = sqlx::query_as::<_, StreamSummary>(concat!(
        summaries_within_retention!(),
        " AND streams.id = $3"
    ))
    .bind(switcher_id)
    .bind(history_days)
    .bind(stream_id)
    .fetch_optional(pool)
    .await?;
    Ok(stream)
}

#[tracing::instrument(skip(pool))]
pub async fn sources(pool: &PgPool, stream_id: Uuid) -> Result<Vec<Source>> {
    let sources = sqlx::query_as::<_, Source>(
        "
        SELECT server_id, name, kind FROM stream_sources
        WHERE stream_id = $1
        ORDER BY created_at, name
        ",
    )
    .bind(stream_id)
    .fetch_all(pool)
    .await?;
    Ok(sources)
}

/// Every server's stats in `[from, to)`, combined into steps of `step_seconds`
/// (a multiple of [`BUCKET_SECONDS`]), ordered by server and time.
#[tracing::instrument(skip(pool))]
pub async fn series(
    pool: &PgPool,
    stream_id: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    step_seconds: i64,
) -> Result<Vec<SeriesPoint>> {
    let points = sqlx::query_as::<_, SeriesPoint>(
        "
        SELECT server_id,
               date_bin(make_interval(secs => $4), bucket_at, $2) AS at,
               sum(bitrate_sum)::float8 / NULLIF(sum(samples), 0) AS bitrate_avg,
               min(bitrate_min) AS bitrate_min,
               max(bitrate_max) AS bitrate_max,
               sum(rtt_sum) / NULLIF(sum(rtt_samples), 0) AS rtt_avg,
               max(rtt_max) AS rtt_max,
               sum(live_seconds)::int8 AS live_seconds,
               sum(low_seconds)::int8 AS low_seconds,
               sum(offline_seconds)::int8 AS offline_seconds
        FROM stream_stats
        WHERE stream_id = $1 AND bucket_at >= $2 AND bucket_at < $3
        GROUP BY server_id, 2
        ORDER BY server_id, 2
        ",
    )
    .bind(stream_id)
    .bind(from)
    .bind(to)
    .bind(step_seconds as f64)
    .fetch_all(pool)
    .await?;
    Ok(points)
}

/// One server's raw stats in `[from, to)`: the last snapshot in each step of
/// `step_seconds`.
#[tracing::instrument(skip(pool))]
pub async fn raw_series(
    pool: &PgPool,
    stream_id: Uuid,
    server_id: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    step_seconds: i64,
) -> Result<Vec<RawPoint>> {
    let points = sqlx::query_as::<_, RawPoint>(
        "
        SELECT DISTINCT ON (date_bin(make_interval(secs => $5), bucket_at, $3))
               bucket_at AS at, raw
        FROM stream_stats
        WHERE stream_id = $1 AND server_id = $2 AND bucket_at >= $3 AND bucket_at < $4
          AND raw IS NOT NULL
        ORDER BY date_bin(make_interval(secs => $5), bucket_at, $3), bucket_at DESC
        ",
    )
    .bind(stream_id)
    .bind(server_id)
    .bind(from)
    .bind(to)
    .bind(step_seconds as f64)
    .fetch_all(pool)
    .await?;
    Ok(points)
}

/// Deletes one of the switcher's streams. Returns whether it existed.
#[tracing::instrument(skip(pool))]
pub async fn delete(pool: &PgPool, switcher_id: Uuid, stream_id: Uuid) -> Result<bool> {
    let result = sqlx::query("DELETE FROM streams WHERE id = $1 AND switcher_id = $2")
        .bind(stream_id)
        .bind(switcher_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Ends open streams with no samples since `idle_since`: their browser source
/// went away and never came back. Returns how many.
#[tracing::instrument(skip(pool))]
pub async fn close_stale(pool: &PgPool, idle_since: DateTime<Utc>) -> Result<u64> {
    let result = sqlx::query(
        "
        UPDATE streams SET ended_at = COALESCE(last_sample_at, started_at)
        WHERE ended_at IS NULL AND COALESCE(last_sample_at, started_at) < $1
        ",
    )
    .bind(idle_since)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Deletes up to `batch` streams that ended longer ago than their workspace
/// plan's `history_days`. Returns how many.
#[tracing::instrument(skip(pool))]
pub async fn delete_expired(pool: &PgPool, batch: i64) -> Result<u64> {
    let result = sqlx::query(
        "
        DELETE FROM streams WHERE id IN (
            SELECT streams.id
            FROM streams
            JOIN switchers ON switchers.id = streams.switcher_id
            JOIN workspaces ON workspaces.id = switchers.workspace_id
            JOIN workspace_plans plans ON plans.workspace_id = workspaces.id
            WHERE streams.ended_at < now() - make_interval(days => plans.history_days)
            LIMIT $1
        )
        ",
    )
    .bind(batch)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Ends one stream at `at` (unless it has ended already).
#[tracing::instrument(skip(pool))]
pub async fn end(pool: &PgPool, stream_id: Uuid, at: DateTime<Utc>) -> Result<()> {
    sqlx::query(
        "
        UPDATE streams SET ended_at = GREATEST($2, started_at)
        WHERE id = $1 AND ended_at IS NULL
        ",
    )
    .bind(stream_id)
    .bind(at)
    .execute(pool)
    .await?;
    Ok(())
}
