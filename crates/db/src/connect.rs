use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    error::Result,
    switchers::{self, Switcher},
};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize, sqlx::Type,
)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ConnectAccess {
    Owners,
    #[default]
    Admins,
    Everyone,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize, sqlx::Type,
)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Watchdog {
    Off,
    #[default]
    Restart,
    RestartResume,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Pairing {
    pub id: Uuid,
    pub device_code_hash: String,
    pub user_code: String,
    pub name: String,
    pub os: String,
    pub app_version: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub switcher_id: Option<Uuid>,
    pub confirmed_by: Option<Uuid>,
    pub confirmed_by_name: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ConnectDevice {
    pub id: Uuid,
    pub switcher_id: Uuid,
    pub name: String,
    pub os: String,
    pub app_version: String,
    pub token_hash: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub enum Claim {
    Expired,
    Pending,
    Claimed {
        device: ConnectDevice,
        confirmed_by_name: Option<String>,
    },
}

pub fn normalize_user_code(code: &str) -> Option<String> {
    let bytes = code.as_bytes();
    let raw = match bytes.len() {
        8 => code.to_owned(),
        9 if bytes[4] == b'-' => format!("{}{}", &code[..4], &code[5..]),
        _ => return None,
    };
    if !raw
        .bytes()
        .all(|byte| b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789".contains(&byte.to_ascii_uppercase()))
    {
        return None;
    }
    Some(raw.to_ascii_uppercase())
}

#[tracing::instrument(skip(pool, device_code_hash))]
pub async fn create_pairing(
    pool: &PgPool,
    device_code_hash: &str,
    user_code: &str,
    name: &str,
    os: &str,
    app_version: &str,
    expires_at: DateTime<Utc>,
) -> Result<Pairing> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM connect_pairings WHERE expires_at <= now()")
        .execute(&mut *tx)
        .await?;
    let pairing = sqlx::query_as::<_, Pairing>(
        "
        INSERT INTO connect_pairings
            (device_code_hash, user_code, name, os, app_version, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING *
        ",
    )
    .bind(device_code_hash)
    .bind(user_code)
    .bind(name)
    .bind(os)
    .bind(app_version)
    .bind(expires_at)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(pairing)
}

#[tracing::instrument(skip(pool))]
pub async fn pairing_by_user_code(pool: &PgPool, user_code: &str) -> Result<Option<Pairing>> {
    let Some(user_code) = normalize_user_code(user_code) else {
        return Ok(None);
    };
    let pairing = sqlx::query_as::<_, Pairing>(
        "SELECT * FROM connect_pairings WHERE user_code = $1 AND expires_at > now()",
    )
    .bind(user_code)
    .fetch_optional(pool)
    .await?;
    Ok(pairing)
}

#[tracing::instrument(skip(pool))]
pub async fn confirm_pairing(
    pool: &PgPool,
    user_code: &str,
    switcher_id: Uuid,
    user_id: Uuid,
    confirmed_by_name: &str,
) -> Result<bool> {
    let Some(user_code) = normalize_user_code(user_code) else {
        return Ok(false);
    };
    let result = sqlx::query(
        "
        UPDATE connect_pairings
        SET switcher_id = $2, confirmed_by = $3, confirmed_by_name = $4
        WHERE user_code = $1 AND expires_at > now() AND switcher_id IS NULL
        ",
    )
    .bind(user_code)
    .bind(switcher_id)
    .bind(user_id)
    .bind(confirmed_by_name)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

#[tracing::instrument(skip(pool, device_code_hash, token_hash))]
pub async fn claim_pairing(
    pool: &PgPool,
    device_code_hash: &str,
    token_hash: &str,
) -> Result<Claim> {
    let mut tx = pool.begin().await?;
    let pairing = sqlx::query_as::<_, Pairing>(
        "SELECT * FROM connect_pairings WHERE device_code_hash = $1 FOR UPDATE",
    )
    .bind(device_code_hash)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(pairing) = pairing.filter(|pairing| pairing.expires_at > Utc::now()) else {
        return Ok(Claim::Expired);
    };
    let Some(switcher_id) = pairing.switcher_id else {
        return Ok(Claim::Pending);
    };

    let device = sqlx::query_as::<_, ConnectDevice>(
        "
        INSERT INTO connect_devices
            (switcher_id, name, os, app_version, token_hash, created_by)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING *
        ",
    )
    .bind(switcher_id)
    .bind(&pairing.name)
    .bind(&pairing.os)
    .bind(&pairing.app_version)
    .bind(token_hash)
    .bind(pairing.confirmed_by)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM connect_pairings WHERE id = $1")
        .bind(pairing.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Claim::Claimed {
        device,
        confirmed_by_name: pairing.confirmed_by_name,
    })
}

#[tracing::instrument(skip(pool))]
pub async fn list_for_switcher(pool: &PgPool, switcher_id: Uuid) -> Result<Vec<ConnectDevice>> {
    let devices = sqlx::query_as::<_, ConnectDevice>(
        "
        SELECT * FROM connect_devices
        WHERE switcher_id = $1 AND revoked_at IS NULL
        ORDER BY created_at DESC, id
        ",
    )
    .bind(switcher_id)
    .fetch_all(pool)
    .await?;
    Ok(devices)
}

#[tracing::instrument(skip(pool, token_hash))]
pub async fn by_token_hash(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<(ConnectDevice, Switcher)>> {
    let device = sqlx::query_as::<_, ConnectDevice>(
        "SELECT * FROM connect_devices WHERE token_hash = $1 AND revoked_at IS NULL",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;
    let Some(device) = device else {
        return Ok(None);
    };
    let Some(switcher) = switchers::get(pool, device.switcher_id).await? else {
        return Ok(None);
    };
    Ok(Some((device, switcher)))
}

#[tracing::instrument(skip(pool))]
pub async fn rename(
    pool: &PgPool,
    switcher_id: Uuid,
    device_id: Uuid,
    name: &str,
) -> Result<Option<ConnectDevice>> {
    let device = sqlx::query_as::<_, ConnectDevice>(
        "
        UPDATE connect_devices SET name = $3
        WHERE switcher_id = $1 AND id = $2 AND revoked_at IS NULL
        RETURNING *
        ",
    )
    .bind(switcher_id)
    .bind(device_id)
    .bind(name)
    .fetch_optional(pool)
    .await?;
    Ok(device)
}

#[tracing::instrument(skip(pool))]
pub async fn revoke(pool: &PgPool, switcher_id: Uuid, device_id: Uuid) -> Result<bool> {
    let result = sqlx::query(
        "
        UPDATE connect_devices SET revoked_at = now()
        WHERE switcher_id = $1 AND id = $2 AND revoked_at IS NULL
        ",
    )
    .bind(switcher_id)
    .bind(device_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

#[tracing::instrument(skip(pool))]
pub async fn touch(pool: &PgPool, device_id: Uuid, os: &str, app_version: &str) -> Result<bool> {
    let result = sqlx::query(
        "
        UPDATE connect_devices
        SET last_seen_at = now(), os = $2, app_version = $3
        WHERE id = $1 AND revoked_at IS NULL
        ",
    )
    .bind(device_id)
    .bind(os)
    .bind(app_version)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
