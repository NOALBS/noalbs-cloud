use chrono::{Duration, Utc};
use db::{connect_logs, connect_logs::ConnectLogKind, switchers, users, workspaces};
use flate2::read::GzDecoder;
use sqlx::PgPool;
use std::io::Read;
use uuid::Uuid;

async fn fixture(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let user = users::create(pool).await.unwrap();
    let workspace = workspaces::create(pool, user.id, "Logs workspace", "logs-workspace")
        .await
        .unwrap();
    let switcher = switchers::create(pool, workspace.id, "Main", b"logs token")
        .await
        .unwrap();
    (user.id, workspace.id, switcher.id)
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn insert_get_and_prune_round_trip_gzip_content(pool: PgPool) {
    let (_, _, switcher_id) = fixture(&pool).await;
    let content = b"OBS log header\nstream started\n";
    let log_id = connect_logs::insert(
        &pool,
        switcher_id,
        None,
        ConnectLogKind::ObsLog,
        "2026-11-04 12-00-00.txt",
        Some(Utc::now()),
        false,
        content,
    )
    .await
    .unwrap();

    let stored: Vec<u8> = sqlx::query_scalar("SELECT content FROM connect_logs WHERE id = $1")
        .bind(log_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let mut decoded = Vec::new();
    GzDecoder::new(stored.as_slice())
        .read_to_end(&mut decoded)
        .unwrap();
    assert_eq!(decoded, content);

    let log = connect_logs::get(&pool, switcher_id, log_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(log.content, String::from_utf8_lossy(content));
    assert_eq!(log.metadata.size_bytes, content.len() as i32);
    assert_eq!(
        connect_logs::list(&pool, switcher_id).await.unwrap().len(),
        1
    );
    assert!(
        connect_logs::delete(&pool, switcher_id, log_id)
            .await
            .unwrap()
    );
    assert!(
        connect_logs::get(&pool, switcher_id, log_id)
            .await
            .unwrap()
            .is_none()
    );

    let stale_id = sqlx::query_scalar::<_, Uuid>(
        "
        INSERT INTO connect_logs
            (switcher_id, kind, file_name, uploaded_at, size_bytes, content)
        VALUES ($1, 'obs_log', 'stale.txt', $2, 0, $3)
        RETURNING id
        ",
    )
    .bind(switcher_id)
    .bind(Utc::now() - Duration::days(31))
    .bind(vec![0x1f_u8, 0x8b])
    .fetch_one(&pool)
    .await
    .unwrap();
    connect_logs::prune(&pool, switcher_id).await.unwrap();
    assert!(
        connect_logs::get(&pool, switcher_id, stale_id)
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test(migrator = "db::MIGRATOR")]
async fn insert_prunes_to_twenty_newest_rows(pool: PgPool) {
    let (_, _, switcher_id) = fixture(&pool).await;
    for index in 0..25 {
        connect_logs::insert(
            &pool,
            switcher_id,
            None,
            ConnectLogKind::CrashReport,
            &format!("crash-{index}.txt"),
            None,
            false,
            format!("content-{index}").as_bytes(),
        )
        .await
        .unwrap();
    }
    let rows = connect_logs::list(&pool, switcher_id).await.unwrap();
    assert_eq!(rows.len(), 20);
    assert!(rows.iter().any(|row| row.file_name == "crash-24.txt"));
    assert!(!rows.iter().any(|row| row.file_name == "crash-0.txt"));
}
