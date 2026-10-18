use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuth2Provider {
    Twitch,
    Kick,
    YouTube,
    /// Local development login, see the API's `dev_login` setting.
    Dev,
}

impl OAuth2Provider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Twitch => "TWITCH",
            Self::Kick => "KICK",
            Self::YouTube => "YOUTUBE",
            Self::Dev => "DEV",
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OAuth2Connection {
    pub id: Uuid,
    pub user_id: Uuid,
}

pub struct UpsertOAuth2Connection<'a> {
    pub provider: OAuth2Provider,
    pub access_token: &'a str,
    pub refresh_token: Option<&'a str>,
    pub token_type: Option<&'a str>,
    pub expires_at: Option<DateTime<Utc>>,
    pub scopes: &'a [String],
    pub provider_id: &'a str,
    pub provider_username: &'a str,
    pub provider_email: Option<&'a str>,
    pub provider_avatar_url: Option<&'a str>,
}

#[tracing::instrument(skip(pool))]
pub async fn get_by_provider_id(
    pool: &PgPool,
    provider: OAuth2Provider,
    provider_id: &str,
) -> Result<Option<OAuth2Connection>> {
    let connection = sqlx::query_as::<_, OAuth2Connection>(
        "
        SELECT id, user_id
        FROM oauth2_connections
        WHERE provider = $1::oauth2_provider AND provider_id = $2
        ",
    )
    .bind(provider.as_str())
    .bind(provider_id)
    .fetch_optional(pool)
    .await?;

    Ok(connection)
}

/// Inserts or refreshes an OAuth connection owned by `user_id`.
///
/// If the provider account already belongs to another user, this returns `Ok(None)`.
/// The conflict condition prevents an account-link request from taking over an
/// existing connection during a concurrent request.
#[tracing::instrument(skip(pool, connection))]
pub async fn upsert_for_user(
    pool: &PgPool,
    user_id: Uuid,
    connection: UpsertOAuth2Connection<'_>,
) -> Result<Option<OAuth2Connection>> {
    let connection = sqlx::query_as::<_, OAuth2Connection>(
        "
        INSERT INTO oauth2_connections (
            user_id, provider, access_token, refresh_token, token_type, expires_at,
            scopes, provider_id, provider_username, provider_email, provider_avatar_url
        )
        VALUES ($1, $2::oauth2_provider, $3, $4, COALESCE($5, 'bearer'), $6, $7, $8, $9, $10, $11)
        ON CONFLICT (provider, provider_id) DO UPDATE SET
            access_token = EXCLUDED.access_token,
            refresh_token = EXCLUDED.refresh_token,
            token_type = EXCLUDED.token_type,
            expires_at = EXCLUDED.expires_at,
            scopes = EXCLUDED.scopes,
            provider_username = EXCLUDED.provider_username,
            provider_email = EXCLUDED.provider_email,
            provider_avatar_url = EXCLUDED.provider_avatar_url
        WHERE oauth2_connections.user_id = EXCLUDED.user_id
        RETURNING id, user_id
        ",
    )
    .bind(user_id)
    .bind(connection.provider.as_str())
    .bind(connection.access_token)
    .bind(connection.refresh_token)
    .bind(connection.token_type)
    .bind(connection.expires_at)
    .bind(connection.scopes)
    .bind(connection.provider_id)
    .bind(connection.provider_username)
    .bind(connection.provider_email)
    .bind(connection.provider_avatar_url)
    .fetch_optional(pool)
    .await?;

    Ok(connection)
}

/// The public part of a login connection, for showing who someone is.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ConnectionProfile {
    pub id: Uuid,
    pub provider: String,
    pub provider_username: String,
    pub provider_avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// The user's connections, most recently used first.
#[tracing::instrument(skip(pool))]
pub async fn list_profiles_for_user(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<ConnectionProfile>> {
    let connections = sqlx::query_as::<_, ConnectionProfile>(
        "
        SELECT id, provider::text AS provider, provider_username, provider_avatar_url, created_at
        FROM oauth2_connections
        WHERE user_id = $1
        ORDER BY updated_at DESC
        ",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(connections)
}

/// What happened to an unlink request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlinkOutcome {
    Unlinked,
    /// Not one of the user's connections.
    NotFound,
    /// It's the user's only way to sign in.
    LastConnection,
}

/// Removes one of the user's login connections. Sessions that signed in with it
/// are signed out, except `current_session`, which moves to another connection
/// so the person unlinking stays signed in.
#[tracing::instrument(skip(pool))]
pub async fn unlink(
    pool: &PgPool,
    user_id: Uuid,
    connection_id: Uuid,
    current_session: Uuid,
) -> Result<UnlinkOutcome> {
    let mut tx = pool.begin().await?;
    // Lock the user's connections so two unlinks can't remove the last two.
    let ids = sqlx::query_scalar::<_, Uuid>(
        "
        SELECT id FROM oauth2_connections
        WHERE user_id = $1
        ORDER BY updated_at DESC
        FOR UPDATE
        ",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;

    if !ids.contains(&connection_id) {
        return Ok(UnlinkOutcome::NotFound);
    }
    let Some(&keep) = ids.iter().find(|&&id| id != connection_id) else {
        return Ok(UnlinkOutcome::LastConnection);
    };

    sqlx::query(
        "
        UPDATE user_sessions SET oauth2_connection_id = $1
        WHERE id = $2 AND oauth2_connection_id = $3
        ",
    )
    .bind(keep)
    .bind(current_session)
    .bind(connection_id)
    .execute(&mut *tx)
    .await?;
    // Other sessions made with this connection go with it (ON DELETE CASCADE).
    sqlx::query("DELETE FROM oauth2_connections WHERE id = $1")
        .bind(connection_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(UnlinkOutcome::Unlinked)
}
