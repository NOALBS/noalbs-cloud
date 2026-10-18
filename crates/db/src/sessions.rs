use chrono::{DateTime, TimeDelta, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{error::Result, users::User};

/// Longest user agent kept; real ones are a few hundred characters.
const MAX_USER_AGENT_LEN: usize = 512;

/// A session ends after this long without being used; each use extends it.
pub const IDLE_LIFETIME: TimeDelta = TimeDelta::days(30);

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserSession {
    pub id: Uuid,
    pub expires_at: DateTime<Utc>,
}

#[tracing::instrument(skip(pool, user_agent))]
pub async fn create(
    pool: &PgPool,
    oauth2_connection_id: Uuid,
    expires_at: DateTime<Utc>,
    user_agent: Option<&str>,
) -> Result<UserSession> {
    let user_agent = user_agent.map(|agent| truncate(agent, MAX_USER_AGENT_LEN));
    let session = sqlx::query_as::<_, UserSession>(
        "
        INSERT INTO user_sessions (oauth2_connection_id, expires_at, user_agent)
        VALUES ($1, $2, $3)
        RETURNING id, expires_at
        ",
    )
    .bind(oauth2_connection_id)
    .bind(expires_at)
    .bind(user_agent)
    .fetch_one(pool)
    .await?;

    Ok(session)
}

fn truncate(value: &str, max: usize) -> &str {
    match value.char_indices().nth(max) {
        Some((index, _)) => &value[..index],
        None => value,
    }
}

#[tracing::instrument(skip(pool))]
pub async fn delete(pool: &PgPool, session_id: Uuid) -> Result<()> {
    sqlx::query("DELETE FROM user_sessions WHERE id = $1")
        .bind(session_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// Looks up a non-expired session and returns its user ID. See
/// [`user_for_active_session`].
pub async fn user_id_for_active_session(pool: &PgPool, session_id: Uuid) -> Result<Option<Uuid>> {
    Ok(user_for_active_session(pool, session_id)
        .await?
        .map(|user| user.id))
}

/// Looks up a non-expired session and returns its user (None for a disabled one).
/// Also marks the session as used, at most every few minutes: that's "last
/// active", and it moves the expiry to [`IDLE_LIFETIME`] from now, so people
/// who keep using the dashboard stay signed in.
#[tracing::instrument(skip(pool, session_id))]
pub async fn user_for_active_session(pool: &PgPool, session_id: Uuid) -> Result<Option<User>> {
    let user = sqlx::query_as::<_, User>(
        "
        WITH session AS (
            SELECT sessions.id, sessions.updated_at, connections.user_id
            FROM user_sessions AS sessions
            JOIN oauth2_connections AS connections
                ON connections.id = sessions.oauth2_connection_id
            JOIN users ON users.id = connections.user_id
            WHERE sessions.id = $1
              AND sessions.expires_at > now()
              AND users.disabled_at IS NULL
        ),
        touched AS (
            UPDATE user_sessions SET updated_at = now(), expires_at = GREATEST(expires_at, $2)
            WHERE id IN (
                SELECT id FROM session WHERE updated_at < now() - interval '5 minutes'
            )
        )
        SELECT users.* FROM session JOIN users ON users.id = session.user_id
        ",
    )
    .bind(session_id)
    .bind(Utc::now() + IDLE_LIFETIME)
    .fetch_optional(pool)
    .await?;

    Ok(user)
}

/// A signed-in browser, for the account page.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionInfo {
    pub id: Uuid,
    /// The connection it signed in with, e.g. `TWITCH`.
    pub provider: String,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// The user's unexpired sessions, most recently used first.
#[tracing::instrument(skip(pool))]
pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> Result<Vec<SessionInfo>> {
    let sessions = sqlx::query_as::<_, SessionInfo>(
        "
        SELECT sessions.id, connections.provider::text AS provider, sessions.user_agent,
               sessions.created_at, sessions.updated_at AS last_used_at, sessions.expires_at
        FROM user_sessions AS sessions
        JOIN oauth2_connections AS connections ON connections.id = sessions.oauth2_connection_id
        WHERE connections.user_id = $1
          AND sessions.expires_at > now()
        ORDER BY sessions.updated_at DESC
        ",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(sessions)
}

/// Signs out one of the user's sessions. `false` if it isn't theirs (or is gone).
#[tracing::instrument(skip(pool))]
pub async fn delete_for_user(pool: &PgPool, user_id: Uuid, session_id: Uuid) -> Result<bool> {
    let deleted = sqlx::query(
        "
        DELETE FROM user_sessions AS sessions
        USING oauth2_connections AS connections
        WHERE connections.id = sessions.oauth2_connection_id
          AND connections.user_id = $1
          AND sessions.id = $2
        ",
    )
    .bind(user_id)
    .bind(session_id)
    .execute(pool)
    .await?
    .rows_affected();

    Ok(deleted > 0)
}

/// Signs out every session of the user except `keep`; returns how many.
#[tracing::instrument(skip(pool))]
pub async fn delete_others(pool: &PgPool, user_id: Uuid, keep: Uuid) -> Result<u64> {
    let deleted = sqlx::query(
        "
        DELETE FROM user_sessions AS sessions
        USING oauth2_connections AS connections
        WHERE connections.id = sessions.oauth2_connection_id
          AND connections.user_id = $1
          AND sessions.id <> $2
        ",
    )
    .bind(user_id)
    .bind(keep)
    .execute(pool)
    .await?
    .rows_affected();

    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncates_on_character_boundaries() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("abcdef", 3), "abc");
        assert_eq!(truncate("ééé", 2), "éé");
    }
}

/// Deletes sessions past their expiry (they hold the browser's user agent).
#[tracing::instrument(skip(pool))]
pub async fn delete_expired(pool: &PgPool) -> Result<u64> {
    let deleted = sqlx::query("DELETE FROM user_sessions WHERE expires_at < now()")
        .execute(pool)
        .await?;
    Ok(deleted.rows_affected())
}

/// Signs the user out everywhere; returns how many sessions ended.
#[tracing::instrument(skip(executor))]
pub async fn delete_all_for_user(
    executor: impl sqlx::PgExecutor<'_>,
    user_id: Uuid,
) -> Result<u64> {
    let deleted = sqlx::query(
        "
        DELETE FROM user_sessions AS sessions
        USING oauth2_connections AS connections
        WHERE connections.id = sessions.oauth2_connection_id
          AND connections.user_id = $1
        ",
    )
    .bind(user_id)
    .execute(executor)
    .await?
    .rows_affected();

    Ok(deleted)
}
