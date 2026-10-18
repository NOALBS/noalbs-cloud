//! NOALBS chat commands: global defaults plus per-switcher overrides and aliases.

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;

/// Languages the chat bot can reply in (the switcher's `language`).
pub const LANGUAGES: &[&str] = &[
    "en", "de", "es", "fr", "it", "nl", "pl", "pt-br", "sv", "tr", "zh-tw",
];

/// Who may use a chat command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "chat_command_roles", rename_all = "UPPERCASE")]
pub enum ChatCommandRole {
    Public,
    Subscriber,
    Vip,
    Moderator,
    Admin,
}

/// A command as configured for one switcher.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SwitcherCommand {
    pub command: String,
    pub description: String,
    pub default_role: ChatCommandRole,
    /// `None` when the switcher has no override (the command is enabled).
    pub enabled: Option<bool>,
    /// `None` when the switcher uses the default role.
    pub min_role: Option<ChatCommandRole>,
    pub aliases: Vec<String>,
}

#[tracing::instrument(skip(pool))]
pub async fn list_for_switcher(pool: &PgPool, switcher_id: Uuid) -> Result<Vec<SwitcherCommand>> {
    let commands = sqlx::query_as::<_, SwitcherCommand>(
        "
        SELECT
            commands.command,
            commands.description,
            commands.default_role,
            overrides.enabled,
            overrides.min_role,
            COALESCE(
                array_agg(aliases.alias ORDER BY aliases.alias)
                    FILTER (WHERE aliases.alias IS NOT NULL),
                '{}'
            ) AS aliases
        FROM noalbs_chat_commands AS commands
        LEFT JOIN noalbs_chat_command_overrides AS overrides
          ON overrides.command = commands.command AND overrides.switcher_id = $1
        LEFT JOIN noalbs_chat_command_aliases AS aliases
          ON aliases.command = commands.command AND aliases.switcher_id = $1
        GROUP BY commands.command, overrides.enabled, overrides.min_role
        ORDER BY commands.command
        ",
    )
    .bind(switcher_id)
    .fetch_all(pool)
    .await?;

    Ok(commands)
}

#[tracing::instrument(skip(pool))]
pub async fn command_exists(pool: &PgPool, command: &str) -> Result<bool> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM noalbs_chat_commands WHERE command = $1)",
    )
    .bind(command)
    .fetch_one(pool)
    .await?;

    Ok(exists)
}

/// Sets whether a command is enabled and who may use it (`None` = its default).
#[tracing::instrument(skip(pool))]
pub async fn set_override(
    pool: &PgPool,
    switcher_id: Uuid,
    command: &str,
    enabled: bool,
    min_role: Option<ChatCommandRole>,
) -> Result<()> {
    sqlx::query(
        "
        INSERT INTO noalbs_chat_command_overrides (switcher_id, command, enabled, min_role)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (switcher_id, command)
        DO UPDATE SET enabled = EXCLUDED.enabled, min_role = EXCLUDED.min_role
        ",
    )
    .bind(switcher_id)
    .bind(command)
    .bind(enabled)
    .bind(min_role)
    .execute(pool)
    .await?;

    Ok(())
}

/// Adds an alias. Fails with a unique violation if the switcher already uses it.
#[tracing::instrument(skip(pool))]
pub async fn add_alias(pool: &PgPool, switcher_id: Uuid, alias: &str, command: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO noalbs_chat_command_aliases (switcher_id, alias, command) VALUES ($1, $2, $3)",
    )
    .bind(switcher_id)
    .bind(alias)
    .bind(command)
    .execute(pool)
    .await?;

    Ok(())
}

#[tracing::instrument(skip(pool))]
pub async fn remove_alias(pool: &PgPool, switcher_id: Uuid, alias: &str) -> Result<bool> {
    let result = sqlx::query(
        "DELETE FROM noalbs_chat_command_aliases WHERE switcher_id = $1 AND alias = $2",
    )
    .bind(switcher_id)
    .bind(alias)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}
