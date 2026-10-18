use sqlx::PgPool;
use uuid::Uuid;

use crate::error::Result;

pub const FREE: &str = "free";

/// A plan and the limits it gives a workspace.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Plan {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub description: String,
    pub is_active: bool,
    pub position: i32,
    pub stripe_product_id: Option<String>,
    pub max_switchers: i32,
    /// Per switcher.
    pub max_stream_servers: i32,
    pub max_workspace_members: i32,
    pub history_days: i32,
    pub custom_chat_bot: bool,
}

/// The columns of [`Plan`], kept as a macro so queries stay `&'static str`.
macro_rules! plan_columns {
    () => {
        "plans.id, plans.code, plans.name, plans.description, plans.is_active, plans.position, \
         plans.stripe_product_id, plans.max_switchers, plans.max_stream_servers, plans.max_workspace_members, plans.history_days, plans.custom_chat_bot"
    };
}

/// What the admin section sets on a plan (the code is fixed once created).
#[derive(Debug, Clone)]
pub struct PlanFields<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub is_active: bool,
    pub position: i32,
    pub max_switchers: i32,
    pub max_stream_servers: i32,
    pub max_workspace_members: i32,
    pub history_days: i32,
    pub custom_chat_bot: bool,
}

/// Lists the plans a workspace can be on, in display order.
#[tracing::instrument(skip(pool))]
pub async fn list_active(pool: &PgPool) -> Result<Vec<Plan>> {
    let plans = sqlx::query_as::<_, Plan>(concat!(
        "SELECT ",
        plan_columns!(),
        " FROM plans WHERE is_active ORDER BY position, max_switchers, code"
    ))
    .fetch_all(pool)
    .await?;

    Ok(plans)
}

/// Every plan, offered or not, in display order.
#[tracing::instrument(skip(pool))]
pub async fn list_all(pool: &PgPool) -> Result<Vec<Plan>> {
    let plans = sqlx::query_as::<_, Plan>(concat!(
        "SELECT ",
        plan_columns!(),
        " FROM plans ORDER BY position, max_switchers, code"
    ))
    .fetch_all(pool)
    .await?;

    Ok(plans)
}

#[tracing::instrument(skip(pool))]
pub async fn get_by_code(pool: &PgPool, code: &str) -> Result<Option<Plan>> {
    let plan = sqlx::query_as::<_, Plan>(concat!(
        "SELECT ",
        plan_columns!(),
        " FROM plans WHERE code = $1"
    ))
    .bind(code)
    .fetch_optional(pool)
    .await?;

    Ok(plan)
}

/// The plan `workspace_id` is currently entitled to.
pub async fn for_workspace<'a>(
    executor: impl sqlx::PgExecutor<'a>,
    workspace_id: Uuid,
) -> Result<Option<Plan>> {
    let plan = sqlx::query_as::<_, Plan>(concat!(
        "SELECT ",
        plan_columns!(),
        " FROM workspace_plans plans WHERE plans.workspace_id = $1"
    ))
    .bind(workspace_id)
    .fetch_optional(executor)
    .await?;

    Ok(plan)
}

/// Creates a plan. A taken code is a unique violation.
#[tracing::instrument(skip(pool, fields))]
pub async fn create(pool: &PgPool, code: &str, fields: &PlanFields<'_>) -> Result<Plan> {
    let plan = sqlx::query_as::<_, Plan>(concat!(
        "
        INSERT INTO plans (code, name, description, is_active, position,
                           max_switchers, max_workspace_members, history_days, custom_chat_bot,
                           max_stream_servers)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        RETURNING ",
        plan_columns!()
    ))
    .bind(code)
    .bind(fields.name)
    .bind(fields.description)
    .bind(fields.is_active)
    .bind(fields.position)
    .bind(fields.max_switchers)
    .bind(fields.max_workspace_members)
    .bind(fields.history_days)
    .bind(fields.custom_chat_bot)
    .bind(fields.max_stream_servers)
    .fetch_one(pool)
    .await?;

    Ok(plan)
}

#[tracing::instrument(skip(pool, fields))]
pub async fn update(pool: &PgPool, code: &str, fields: &PlanFields<'_>) -> Result<Option<Plan>> {
    let plan = sqlx::query_as::<_, Plan>(concat!(
        "
        UPDATE plans SET
            name = $2, description = $3, is_active = $4, position = $5,
            max_switchers = $6, max_workspace_members = $7, history_days = $8,
            custom_chat_bot = $9, max_stream_servers = $10
        WHERE code = $1
        RETURNING ",
        plan_columns!()
    ))
    .bind(code)
    .bind(fields.name)
    .bind(fields.description)
    .bind(fields.is_active)
    .bind(fields.position)
    .bind(fields.max_switchers)
    .bind(fields.max_workspace_members)
    .bind(fields.history_days)
    .bind(fields.custom_chat_bot)
    .bind(fields.max_stream_servers)
    .fetch_optional(pool)
    .await?;

    Ok(plan)
}

#[tracing::instrument(skip(pool))]
pub async fn set_stripe_product(pool: &PgPool, code: &str, product_id: &str) -> Result<()> {
    sqlx::query("UPDATE plans SET stripe_product_id = $2 WHERE code = $1")
        .bind(code)
        .bind(product_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// The workspaces on a plan (not client workspaces: their limits are their
/// organization's), e.g. to re-check paused switchers after its limits change.
#[tracing::instrument(skip(pool))]
pub async fn workspace_ids(pool: &PgPool, code: &str) -> Result<Vec<Uuid>> {
    let ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM workspaces WHERE plan_code = $1 AND organization_id IS NULL",
    )
    .bind(code)
    .fetch_all(pool)
    .await?;
    Ok(ids)
}
