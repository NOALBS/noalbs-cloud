//! Server combinations ("scene rules"): scenes to use while all of a set of a
//! switcher's stream servers have a feed. See obs-link's `decide::Rule`.

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::error::Result;

/// How many rules a switcher may have.
pub const MAX_SCENE_RULES: i64 = 20;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SceneRule {
    pub id: Uuid,
    pub switcher_id: Uuid,

    pub priority: i32,
    pub enabled: bool,

    pub scene_live: Option<Uuid>,
    pub scene_low: Option<Uuid>,
    pub scene_offline: Option<Uuid>,

    /// The rule's servers, in their priority order.
    pub server_ids: Vec<Uuid>,

    pub created_at: DateTime<chrono::Utc>,
    pub updated_at: DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct SceneRuleInput {
    pub server_ids: Vec<Uuid>,
    pub enabled: bool,
    pub scene_live: Option<Uuid>,
    pub scene_low: Option<Uuid>,
    pub scene_offline: Option<Uuid>,
}

#[derive(Debug)]
pub enum SaveOutcome {
    Saved(SceneRule),
    /// The rule isn't the switcher's.
    NotFound,
    /// A server isn't the switcher's.
    UnknownServer,
    /// Another rule has the same servers.
    Duplicate,
    /// The switcher has [`MAX_SCENE_RULES`] already.
    TooMany,
}

/// The rule columns plus `server_ids`, followed by `$tail` (a WHERE and GROUP BY).
macro_rules! select {
    ($tail:literal) => {
        concat!(
            "
    SELECT
        rules.*,
        COALESCE(
            array_agg(members.stream_server_id ORDER BY servers.priority, servers.id)
                FILTER (WHERE members.stream_server_id IS NOT NULL),
            '{}'
        ) AS server_ids
    FROM scene_rules AS rules
    LEFT JOIN scene_rule_servers AS members ON members.scene_rule_id = rules.id
    LEFT JOIN stream_servers AS servers ON servers.id = members.stream_server_id
    ",
            $tail
        )
    };
}

/// The switcher's rules, first tried first.
#[tracing::instrument(skip(pool))]
pub async fn by_switcher_id(pool: &PgPool, switcher_id: Uuid) -> Result<Vec<SceneRule>> {
    let rules = sqlx::query_as::<_, SceneRule>(select!(
        "WHERE rules.switcher_id = $1
        GROUP BY rules.id
        ORDER BY rules.priority, rules.created_at, rules.id"
    ))
    .bind(switcher_id)
    .fetch_all(pool)
    .await?;

    Ok(rules)
}

async fn one(conn: &mut PgConnection, rule_id: Uuid) -> Result<SceneRule> {
    let rule = sqlx::query_as::<_, SceneRule>(select!("WHERE rules.id = $1 GROUP BY rules.id"))
        .bind(rule_id)
        .fetch_one(conn)
        .await?;

    Ok(rule)
}

/// Checks `servers` (sorted, deduplicated) against the switcher's servers and
/// its other rules. Expects the switcher row to be locked.
async fn check_servers(
    conn: &mut PgConnection,
    switcher_id: Uuid,
    servers: &[Uuid],
    except_rule: Option<Uuid>,
) -> Result<Option<SaveOutcome>> {
    let known: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM stream_servers WHERE switcher_id = $1 AND id = ANY($2)",
    )
    .bind(switcher_id)
    .bind(servers)
    .fetch_one(&mut *conn)
    .await?;
    if usize::try_from(known).ok() != Some(servers.len()) {
        return Ok(Some(SaveOutcome::UnknownServer));
    }

    let sets: Vec<Vec<Uuid>> = sqlx::query_scalar(
        "
        SELECT array_agg(members.stream_server_id ORDER BY members.stream_server_id)
        FROM scene_rules AS rules
        JOIN scene_rule_servers AS members ON members.scene_rule_id = rules.id
        WHERE rules.switcher_id = $1 AND rules.id IS DISTINCT FROM $2
        GROUP BY rules.id
        ",
    )
    .bind(switcher_id)
    .bind(except_rule)
    .fetch_all(&mut *conn)
    .await?;
    if sets.iter().any(|set| set == servers) {
        return Ok(Some(SaveOutcome::Duplicate));
    }

    Ok(None)
}

async fn set_servers(conn: &mut PgConnection, rule_id: Uuid, servers: &[Uuid]) -> Result<()> {
    sqlx::query("DELETE FROM scene_rule_servers WHERE scene_rule_id = $1")
        .bind(rule_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "
        INSERT INTO scene_rule_servers (scene_rule_id, stream_server_id)
        SELECT $1, unnest($2::uuid[])
        ",
    )
    .bind(rule_id)
    .bind(servers)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

fn sorted(ids: &[Uuid]) -> Vec<Uuid> {
    let mut ids = ids.to_vec();
    ids.sort();
    ids.dedup();
    ids
}

/// Locks the switcher row, so concurrent saves can't add the same set twice
/// or go over the limit, and a server deletion can't interleave with a save
/// (see [`crate::stream_servers::delete`]). Returns `false` if it doesn't exist.
pub(crate) async fn lock_switcher(conn: &mut PgConnection, switcher_id: Uuid) -> Result<bool> {
    let found = sqlx::query_scalar::<_, Uuid>("SELECT id FROM switchers WHERE id = $1 FOR UPDATE")
        .bind(switcher_id)
        .fetch_optional(conn)
        .await?;
    Ok(found.is_some())
}

/// Adds a rule at the end of the switcher's order.
#[tracing::instrument(skip(pool))]
pub async fn create(
    pool: &PgPool,
    switcher_id: Uuid,
    input: &SceneRuleInput,
) -> Result<SaveOutcome> {
    let servers = sorted(&input.server_ids);
    let mut tx = pool.begin().await?;
    if !lock_switcher(&mut tx, switcher_id).await? {
        return Ok(SaveOutcome::NotFound);
    }

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM scene_rules WHERE switcher_id = $1")
        .bind(switcher_id)
        .fetch_one(&mut *tx)
        .await?;
    if count >= MAX_SCENE_RULES {
        return Ok(SaveOutcome::TooMany);
    }
    if let Some(refused) = check_servers(&mut tx, switcher_id, &servers, None).await? {
        return Ok(refused);
    }

    let rule_id: Uuid = sqlx::query_scalar(
        "
        INSERT INTO scene_rules (switcher_id, priority, enabled, scene_live, scene_low, scene_offline)
        SELECT $1, COALESCE(max(priority) + 1, 0), $2, $3, $4, $5
        FROM scene_rules WHERE switcher_id = $1
        RETURNING id
        ",
    )
    .bind(switcher_id)
    .bind(input.enabled)
    .bind(input.scene_live)
    .bind(input.scene_low)
    .bind(input.scene_offline)
    .fetch_one(&mut *tx)
    .await?;
    set_servers(&mut tx, rule_id, &servers).await?;
    let rule = one(&mut tx, rule_id).await?;

    tx.commit().await?;
    Ok(SaveOutcome::Saved(rule))
}

/// Replaces a rule's servers, scenes and enabled flag.
#[tracing::instrument(skip(pool))]
pub async fn update(
    pool: &PgPool,
    switcher_id: Uuid,
    rule_id: Uuid,
    input: &SceneRuleInput,
) -> Result<SaveOutcome> {
    let servers = sorted(&input.server_ids);
    let mut tx = pool.begin().await?;
    if !lock_switcher(&mut tx, switcher_id).await? {
        return Ok(SaveOutcome::NotFound);
    }
    if let Some(refused) = check_servers(&mut tx, switcher_id, &servers, Some(rule_id)).await? {
        return Ok(refused);
    }

    let updated = sqlx::query(
        "
        UPDATE scene_rules
        SET enabled = $3, scene_live = $4, scene_low = $5, scene_offline = $6
        WHERE id = $1 AND switcher_id = $2
        ",
    )
    .bind(rule_id)
    .bind(switcher_id)
    .bind(input.enabled)
    .bind(input.scene_live)
    .bind(input.scene_low)
    .bind(input.scene_offline)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Ok(SaveOutcome::NotFound);
    }
    set_servers(&mut tx, rule_id, &servers).await?;
    let rule = one(&mut tx, rule_id).await?;

    tx.commit().await?;
    Ok(SaveOutcome::Saved(rule))
}

#[tracing::instrument(skip(pool))]
pub async fn delete(pool: &PgPool, switcher_id: Uuid, rule_id: Uuid) -> Result<bool> {
    let result = sqlx::query("DELETE FROM scene_rules WHERE id = $1 AND switcher_id = $2")
        .bind(rule_id)
        .bind(switcher_id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Sets the order: `ids[0]` is tried first. Returns `false`, changing nothing,
/// unless `ids` is exactly the switcher's rules.
#[tracing::instrument(skip(pool))]
pub async fn reorder(pool: &PgPool, switcher_id: Uuid, ids: &[Uuid]) -> Result<bool> {
    let mut tx = pool.begin().await?;

    let mut current = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM scene_rules WHERE switcher_id = $1 FOR UPDATE",
    )
    .bind(switcher_id)
    .fetch_all(&mut *tx)
    .await?;
    current.sort();
    if current != sorted(ids) || current.len() != ids.len() {
        return Ok(false);
    }

    sqlx::query(
        "
        UPDATE scene_rules
        SET priority = ordered.position - 1
        FROM unnest($2::uuid[]) WITH ORDINALITY AS ordered(id, position)
        WHERE scene_rules.id = ordered.id AND scene_rules.switcher_id = $1
        ",
    )
    .bind(switcher_id)
    .bind(ids)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(true)
}
