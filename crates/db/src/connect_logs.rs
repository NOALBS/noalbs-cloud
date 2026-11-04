use std::io::{Read, Write};

use chrono::{DateTime, Utc};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, sqlx::Type)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ConnectLogKind {
    ObsLog,
    CrashReport,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ConnectLogMetadata {
    pub id: Uuid,
    pub kind: ConnectLogKind,
    pub file_name: String,
    pub file_modified_at: Option<DateTime<Utc>>,
    pub uploaded_at: DateTime<Utc>,
    pub size_bytes: i32,
    pub truncated: bool,
    pub device_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConnectLog {
    pub metadata: ConnectLogMetadata,
    pub content: String,
}

#[tracing::instrument(skip(pool, content))]
pub async fn insert(
    pool: &PgPool,
    switcher_id: Uuid,
    device_id: Option<Uuid>,
    kind: ConnectLogKind,
    file_name: &str,
    file_modified_at: Option<DateTime<Utc>>,
    truncated: bool,
    content: &[u8],
) -> Result<Uuid> {
    let size_bytes = i32::try_from(content.len()).map_err(|_| {
        Error::Sqlx(sqlx::Error::Protocol(
            "Connect log is too large to store".into(),
        ))
    })?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(content)
        .map_err(|error| Error::Sqlx(sqlx::Error::Io(error)))?;
    let compressed = encoder
        .finish()
        .map_err(|error| Error::Sqlx(sqlx::Error::Io(error)))?;
    let id: Uuid = sqlx::query_scalar(
        "
        INSERT INTO connect_logs
            (switcher_id, device_id, kind, file_name, file_modified_at, size_bytes, truncated, content)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id
        ",
    )
    .bind(switcher_id)
    .bind(device_id)
    .bind(kind)
    .bind(file_name)
    .bind(file_modified_at)
    .bind(size_bytes)
    .bind(truncated)
    .bind(compressed)
    .fetch_one(pool)
    .await?;
    prune(pool, switcher_id).await?;
    Ok(id)
}

#[tracing::instrument(skip(pool))]
pub async fn list(pool: &PgPool, switcher_id: Uuid) -> Result<Vec<ConnectLogMetadata>> {
    Ok(sqlx::query_as(
        "
        SELECT log.id, log.kind, log.file_name, log.file_modified_at, log.uploaded_at,
            log.size_bytes, log.truncated, device.name AS device_name
        FROM connect_logs AS log
        LEFT JOIN connect_devices AS device ON device.id = log.device_id
        WHERE log.switcher_id = $1
        ORDER BY log.uploaded_at DESC, log.id DESC
        ",
    )
    .bind(switcher_id)
    .fetch_all(pool)
    .await?)
}

#[tracing::instrument(skip(pool))]
pub async fn get(pool: &PgPool, switcher_id: Uuid, id: Uuid) -> Result<Option<ConnectLog>> {
    let row: Option<(
        Uuid,
        ConnectLogKind,
        String,
        Option<DateTime<Utc>>,
        DateTime<Utc>,
        i32,
        bool,
        Option<String>,
        Vec<u8>,
    )> = sqlx::query_as(
        "
        SELECT log.id, log.kind, log.file_name, log.file_modified_at, log.uploaded_at,
            log.size_bytes, log.truncated, device.name AS device_name, log.content
        FROM connect_logs AS log
        LEFT JOIN connect_devices AS device ON device.id = log.device_id
        WHERE log.switcher_id = $1 AND log.id = $2
        ",
    )
    .bind(switcher_id)
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(
        |(
            id,
            kind,
            file_name,
            file_modified_at,
            uploaded_at,
            size_bytes,
            truncated,
            device_name,
            compressed,
        )| {
            let mut decoder = GzDecoder::new(compressed.as_slice());
            let mut content = Vec::new();
            decoder
                .read_to_end(&mut content)
                .map_err(|error| Error::Sqlx(sqlx::Error::Io(error)))?;
            Ok(ConnectLog {
                metadata: ConnectLogMetadata {
                    id,
                    kind,
                    file_name,
                    file_modified_at,
                    uploaded_at,
                    size_bytes,
                    truncated,
                    device_name,
                },
                content: String::from_utf8_lossy(&content).into_owned(),
            })
        },
    )
    .transpose()
}

#[tracing::instrument(skip(pool))]
pub async fn delete(pool: &PgPool, switcher_id: Uuid, id: Uuid) -> Result<bool> {
    let result = sqlx::query("DELETE FROM connect_logs WHERE switcher_id = $1 AND id = $2")
        .bind(switcher_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[tracing::instrument(skip(pool))]
pub async fn prune(pool: &PgPool, switcher_id: Uuid) -> Result<()> {
    sqlx::query(
        "
        WITH ranked AS (
            SELECT id, row_number() OVER (ORDER BY uploaded_at DESC, id DESC) AS position
            FROM connect_logs
            WHERE switcher_id = $1
        )
        DELETE FROM connect_logs
        WHERE switcher_id = $1
          AND (
              uploaded_at < now() - interval '30 days'
              OR id IN (SELECT id FROM ranked WHERE position > 20)
          )
        ",
    )
    .bind(switcher_id)
    .execute(pool)
    .await?;
    Ok(())
}
