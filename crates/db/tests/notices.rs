//! Notices: what is in force, and the schedule rules.

use chrono::{DateTime, TimeDelta, Utc};
use db::notices::{self, NoticeFields};
use db::users;
use sqlx::PgPool;

fn fields(
    message: &str,
    starts_at: DateTime<Utc>,
    ends_at: Option<DateTime<Utc>>,
) -> NoticeFields<'_> {
    NoticeFields {
        message,
        severity: "info",
        starts_at,
        ends_at,
        dismissible: true,
    }
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn only_notices_in_force_are_active(pool: PgPool) {
    let staff = users::create(&pool).await.unwrap().id;
    let now = Utc::now();
    let hour = TimeDelta::hours(1);

    for (message, starts, ends) in [
        ("open ended", now - hour, None),
        ("running", now - hour, Some(now + hour)),
        ("upcoming", now + hour, None),
        ("ended", now - hour * 2, Some(now - hour)),
    ] {
        notices::create(&pool, &fields(message, starts, ends), staff, "staff")
            .await
            .unwrap();
    }

    let active: Vec<_> = notices::active(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|notice| notice.message)
        .collect();
    assert_eq!(active, ["open ended", "running"]);
    assert_eq!(notices::list(&pool, 10).await.unwrap().len(), 4);
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn a_notice_cannot_end_before_it_starts(pool: PgPool) {
    let staff = users::create(&pool).await.unwrap().id;
    let now = Utc::now();

    let result = notices::create(&pool, &fields("backwards", now, Some(now)), staff, "staff").await;
    assert!(result.is_err());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn ending_stops_a_running_notice_only(pool: PgPool) {
    let staff = users::create(&pool).await.unwrap().id;
    let now = Utc::now();
    let hour = TimeDelta::hours(1);

    let running = notices::create(&pool, &fields("running", now - hour, None), staff, "staff")
        .await
        .unwrap();
    let upcoming = notices::create(&pool, &fields("upcoming", now + hour, None), staff, "staff")
        .await
        .unwrap();

    assert!(notices::end_now(&pool, running.id).await.unwrap().is_some());
    assert!(notices::end_now(&pool, running.id).await.unwrap().is_none());
    assert!(
        notices::end_now(&pool, upcoming.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(notices::active(&pool).await.unwrap().is_empty());

    assert!(notices::delete(&pool, upcoming.id).await.unwrap());
    assert!(!notices::delete(&pool, upcoming.id).await.unwrap());
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn update_changes_the_fields(pool: PgPool) {
    let staff = users::create(&pool).await.unwrap().id;
    let now = Utc::now();
    let notice = notices::create(&pool, &fields("first", now, None), staff, "staff")
        .await
        .unwrap();

    let changed = NoticeFields {
        severity: "critical",
        dismissible: false,
        ..fields("second", now, None)
    };
    let updated = notices::update(&pool, notice.id, &changed)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.message, "second");
    assert_eq!(updated.severity, "critical");
    assert!(!updated.dismissible);
    assert_eq!(updated.created_by_name.as_deref(), Some("staff"));
}
