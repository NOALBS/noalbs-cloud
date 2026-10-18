//! Stream history: starting, resuming and ending streams, their stats buckets,
//! and keeping them within the plan's retention.

use chrono::{DateTime, SubsecRound, TimeDelta, Utc};
use db::{
    stream_servers::StreamServerKind,
    streams::{self, Bucket, Source, StateSeconds},
    switchers, users, workspaces,
};
use sqlx::PgPool;
use uuid::Uuid;

/// Whole seconds, so times survive Postgres's microseconds unchanged.
fn now() -> DateTime<Utc> {
    Utc::now().trunc_subsecs(0)
}

/// A switcher in a new (free) workspace: `(workspace_id, switcher_id)`.
async fn switcher(pool: &PgPool) -> (Uuid, Uuid) {
    let owner = users::create(pool).await.unwrap();
    let workspace = workspaces::create(pool, owner.id, "Nomad Live", "nomad-live")
        .await
        .unwrap();
    let switcher = switchers::create(pool, workspace.id, "Main", b"a")
        .await
        .unwrap();
    (workspace.id, switcher.id)
}

fn bucket(server_id: Uuid, at: DateTime<Utc>, bitrate: i32) -> Bucket {
    Bucket {
        server_id,
        bucket_at: at,
        samples: 1,
        bitrate_sum: i64::from(bitrate),
        bitrate_min: bitrate,
        bitrate_max: bitrate,
        rtt_sum: 0.0,
        rtt_samples: 0,
        rtt_max: None,
        live_seconds: 1,
        low_seconds: 0,
        offline_seconds: 0,
        raw: Some(serde_json::json!({ "bitrate": bitrate })),
    }
}

async fn ended_at(pool: &PgPool, stream_id: Uuid) -> Option<DateTime<Utc>> {
    sqlx::query_scalar("SELECT ended_at FROM streams WHERE id = $1")
        .bind(stream_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn a_new_stream_closes_the_open_one(pool: PgPool) {
    let (_, switcher_id) = switcher(&pool).await;
    let t0 = now() - TimeDelta::hours(3);

    let first = streams::start(&pool, switcher_id, t0).await.unwrap();
    let second = streams::start(&pool, switcher_id, t0 + TimeDelta::hours(1))
        .await
        .unwrap();

    assert_ne!(first.id, second.id);
    // No samples, so it ends where it started.
    assert_eq!(ended_at(&pool, first.id).await, Some(first.started_at));
    assert_eq!(ended_at(&pool, second.id).await, None);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn resume_finds_the_same_stream_even_after_it_was_closed(pool: PgPool) {
    let (_, switcher_id) = switcher(&pool).await;
    let t0 = now() - TimeDelta::hours(1);
    let stream = streams::start(&pool, switcher_id, t0).await.unwrap();

    // obs-link reconnects: OBS's output duration gives a slightly different start.
    let resumed = streams::resume(&pool, switcher_id, t0 + TimeDelta::seconds(3))
        .await
        .unwrap();
    assert_eq!(resumed.id, stream.id);

    // The upkeep closed it while the browser source was away; it reopens.
    streams::end_open(&pool, switcher_id, None).await.unwrap();
    let reopened = streams::resume(&pool, switcher_id, t0).await.unwrap();
    assert_eq!(reopened.id, stream.id);
    assert_eq!(reopened.ended_at, None);

    // A stream that started well after is a new one.
    let other = streams::resume(&pool, switcher_id, t0 + TimeDelta::minutes(30))
        .await
        .unwrap();
    assert_ne!(other.id, stream.id);
    assert!(ended_at(&pool, stream.id).await.is_some());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn buckets_written_twice_merge(pool: PgPool) {
    let (_, switcher_id) = switcher(&pool).await;
    let t0 = now() - TimeDelta::minutes(10);
    let stream = streams::start(&pool, switcher_id, t0).await.unwrap();
    let server_id = Uuid::new_v4();
    streams::upsert_sources(
        &pool,
        stream.id,
        &[Source {
            server_id,
            name: "BELABOX Cloud".into(),
            kind: StreamServerKind::Belabox,
        }],
    )
    .await
    .unwrap();

    let at = t0 + TimeDelta::seconds(20);
    let states = StateSeconds {
        live: 1,
        low: 0,
        offline: 0,
    };
    let mut rtt = bucket(server_id, at, 6000);
    rtt.rtt_sum = 120.0;
    rtt.rtt_samples = 1;
    rtt.rtt_max = Some(120.0);
    streams::write_buckets(&pool, stream.id, &[rtt], states, at)
        .await
        .unwrap();
    // The same bucket again, after a switcher restart.
    streams::write_buckets(&pool, stream.id, &[bucket(server_id, at, 2000)], states, at)
        .await
        .unwrap();

    let series = streams::series(&pool, stream.id, t0, now(), 10)
        .await
        .unwrap();
    assert_eq!(series.len(), 1);
    let point = &series[0];
    assert_eq!(point.bitrate_avg, Some(4000.0));
    assert_eq!((point.bitrate_min, point.bitrate_max), (2000, 6000));
    assert_eq!((point.rtt_avg, point.rtt_max), (Some(120.0), Some(120.0)));
    assert_eq!(point.live_seconds, 2);

    let raw = streams::raw_series(&pool, stream.id, server_id, t0, now(), 10)
        .await
        .unwrap();
    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0].raw, serde_json::json!({ "bitrate": 2000 }));

    let summary = streams::get(&pool, switcher_id, 7, stream.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(summary.stream.live_seconds, 2);
    assert_eq!(summary.stream.last_sample_at, Some(at));

    let sources = streams::sources(&pool, stream.id).await.unwrap();
    assert_eq!(sources.len(), 1);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn series_combines_buckets_into_steps(pool: PgPool) {
    let (_, switcher_id) = switcher(&pool).await;
    let t0 = DateTime::parse_from_rfc3339("2026-09-01T20:00:00Z")
        .unwrap()
        .to_utc();
    let stream = streams::start(&pool, switcher_id, t0).await.unwrap();
    let server_id = Uuid::new_v4();
    let buckets: Vec<Bucket> = (0..6)
        .map(|i| {
            bucket(
                server_id,
                t0 + TimeDelta::seconds(i * 10),
                1000 * (i as i32 + 1),
            )
        })
        .collect();
    streams::write_buckets(&pool, stream.id, &buckets, StateSeconds::default(), t0)
        .await
        .unwrap();

    let series = streams::series(&pool, stream.id, t0, t0 + TimeDelta::minutes(1), 30)
        .await
        .unwrap();
    assert_eq!(series.len(), 2);
    assert_eq!(series[0].at, t0);
    assert_eq!(series[0].bitrate_avg, Some(2000.0));
    assert_eq!((series[1].bitrate_min, series[1].bitrate_max), (4000, 6000));

    // The raw series keeps the last snapshot of each step.
    let raw = streams::raw_series(
        &pool,
        stream.id,
        server_id,
        t0,
        t0 + TimeDelta::minutes(1),
        30,
    )
    .await
    .unwrap();
    assert_eq!(
        raw.iter()
            .map(|point| point.raw["bitrate"].as_i64())
            .collect::<Vec<_>>(),
        [Some(3000), Some(6000)]
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn history_is_kept_for_the_plans_days(pool: PgPool) {
    let (workspace_id, switcher_id) = switcher(&pool).await;
    let now = now();
    let old = streams::start(&pool, switcher_id, now - TimeDelta::days(9))
        .await
        .unwrap();
    streams::end_open(&pool, switcher_id, Some(now - TimeDelta::days(8)))
        .await
        .unwrap();
    let recent = streams::start(&pool, switcher_id, now - TimeDelta::hours(2))
        .await
        .unwrap();

    // Free keeps 7 days: the old stream is hidden right away.
    let listed = streams::list(&pool, switcher_id, 7, None, 50)
        .await
        .unwrap();
    assert_eq!(
        listed.iter().map(|s| s.stream.id).collect::<Vec<_>>(),
        [recent.id]
    );
    assert!(
        streams::get(&pool, switcher_id, 7, old.id)
            .await
            .unwrap()
            .is_none()
    );

    // On pro it's still kept, so the upkeep leaves it.
    workspaces::set_plan(&pool, workspace_id, "pro")
        .await
        .unwrap();
    assert_eq!(streams::delete_expired(&pool, 100).await.unwrap(), 0);

    // Back on free, the upkeep deletes it; the open stream stays.
    workspaces::set_plan(&pool, workspace_id, "free")
        .await
        .unwrap();
    assert_eq!(streams::delete_expired(&pool, 100).await.unwrap(), 1);
    let listed = streams::list(&pool, switcher_id, 90, None, 50)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn stale_open_streams_are_closed(pool: PgPool) {
    let (_, switcher_id) = switcher(&pool).await;
    let now = now();
    let stream = streams::start(&pool, switcher_id, now - TimeDelta::hours(2))
        .await
        .unwrap();
    let last = now - TimeDelta::hours(1);
    streams::write_buckets(&pool, stream.id, &[], StateSeconds::default(), last)
        .await
        .unwrap();

    let closed = streams::close_stale(&pool, now - TimeDelta::minutes(30))
        .await
        .unwrap();
    assert_eq!(closed, 1);
    let ended = ended_at(&pool, stream.id).await.unwrap();
    assert!((ended - last).abs() < TimeDelta::milliseconds(1));
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn streams_are_listed_newest_first_and_page_back(pool: PgPool) {
    let (_, switcher_id) = switcher(&pool).await;
    let now = now();
    for hours in [5, 3, 1] {
        streams::start(&pool, switcher_id, now - TimeDelta::hours(hours))
            .await
            .unwrap();
    }

    let first = streams::list(&pool, switcher_id, 7, None, 2).await.unwrap();
    assert_eq!(first.len(), 2);
    assert!(first[0].stream.started_at > first[1].stream.started_at);
    let rest = streams::list(&pool, switcher_id, 7, Some(first[1].stream.started_at), 2)
        .await
        .unwrap();
    assert_eq!(rest.len(), 1);

    assert!(
        streams::delete(&pool, switcher_id, rest[0].stream.id)
            .await
            .unwrap()
    );
    assert!(
        !streams::delete(&pool, switcher_id, rest[0].stream.id)
            .await
            .unwrap()
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn open_is_the_running_stream_only(pool: PgPool) {
    let (_, switcher_id) = switcher(&pool).await;
    assert!(streams::open(&pool, switcher_id).await.unwrap().is_none());

    let stream = streams::start(&pool, switcher_id, now()).await.unwrap();
    let open = streams::open(&pool, switcher_id).await.unwrap().unwrap();
    assert_eq!(open.id, stream.id);

    streams::end(&pool, stream.id, now()).await.unwrap();
    assert!(streams::open(&pool, switcher_id).await.unwrap().is_none());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn markers_come_back_in_order_and_go_with_the_stream(pool: PgPool) {
    use db::stream_markers;

    let (_, switcher_id) = switcher(&pool).await;
    let start = now();
    let stream = streams::start(&pool, switcher_id, start).await.unwrap();

    let later = start + TimeDelta::minutes(5);
    stream_markers::insert(&pool, stream.id, later, None, None)
        .await
        .unwrap();
    stream_markers::insert(
        &pool,
        stream.id,
        start + TimeDelta::minutes(1),
        Some("Clutch"),
        Some("Mod (Twitch chat)"),
    )
    .await
    .unwrap();

    let markers = stream_markers::list(&pool, stream.id).await.unwrap();
    assert_eq!(markers.len(), 2);
    assert_eq!(markers[0].description.as_deref(), Some("Clutch"));
    assert_eq!(markers[0].created_by.as_deref(), Some("Mod (Twitch chat)"));
    assert_eq!(markers[1].at, later);
    assert_eq!(markers[1].description, None);

    assert!(
        streams::delete(&pool, switcher_id, stream.id)
            .await
            .unwrap()
    );
    assert!(
        stream_markers::list(&pool, stream.id)
            .await
            .unwrap()
            .is_empty()
    );
}
