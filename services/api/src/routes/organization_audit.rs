//! An organization's audit log: what its team changed in the organization and in its
//! clients, what its customers did in their clients (their joining, inviting and
//! removing people, renaming), and what NOALBS staff did to the organization. Admins
//! read it; every handler that changes something calls [`record`] or one of its
//! variants after it worked. The log is kept a year (`privacy::upkeep`).

use axum::{
    Json,
    extract::{Query, State},
};
use chrono::{DateTime, Utc};
use db::organization_audit::{ActorKind, NewEntry};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{
        OrgManage, OrganizationAccess, OrganizationAuthorized, Via, WorkspaceAccess,
        user_display_name,
    },
    routes::{ApiResult, ErrorBody},
};

const DEFAULT_ENTRIES: i64 = 50;
const MAX_ENTRIES: i64 = 200;

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new().routes(routes!(organization_audit))
}

/// Records what `user_id` did, named as they are called now.
pub(crate) async fn record(
    state: &ApiState,
    organization_id: Uuid,
    user_id: Uuid,
    kind: ActorKind,
    action: &str,
    subject: Option<&str>,
    details: serde_json::Value,
) -> ApiResult<()> {
    let actor_name = user_display_name(state, user_id).await?;
    db::organization_audit::record(
        &state.db,
        &NewEntry {
            organization_id,
            actor_user_id: user_id,
            actor_name: &actor_name,
            actor_kind: kind,
            action,
            subject,
            details,
        },
    )
    .await?;
    Ok(())
}

/// Records something the team did in the organization.
pub(crate) async fn record_team(
    state: &ApiState,
    access: &OrganizationAccess,
    action: &str,
    subject: Option<&str>,
    details: serde_json::Value,
) -> ApiResult<()> {
    record(
        state,
        access.organization_id,
        access.user.id(),
        ActorKind::Team,
        action,
        subject,
        details,
    )
    .await
}

/// Records something done in a client workspace by its team or by a customer. Does
/// nothing for an ordinary workspace, or for NOALBS support (its own log has it).
pub(crate) async fn record_in_client(
    state: &ApiState,
    access: &WorkspaceAccess,
    action: &str,
    subject: Option<&str>,
    details: serde_json::Value,
) -> ApiResult<()> {
    let kind = match access.via {
        Via::Organization => ActorKind::Team,
        Via::Member => ActorKind::Customer,
        Via::Support { .. } => return Ok(()),
    };
    let Some(workspace) = db::workspaces::get_by_id(&state.db, access.workspace_id).await? else {
        return Ok(());
    };
    let Some(organization_id) = workspace.organization_id else {
        return Ok(());
    };
    let subject = subject.unwrap_or(&workspace.name);
    // Which client it was in, when the subject is something else.
    let mut details = details;
    if let Some(object) = details.as_object_mut() {
        object.insert("client".to_owned(), workspace.name.clone().into());
    }
    record(
        state,
        organization_id,
        access.user.id(),
        kind,
        action,
        Some(subject),
        details,
    )
    .await
}

/// Who did it.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditActor {
    /// The organization's team.
    Team,
    /// A customer, in their client workspace.
    Customer,
    /// NOALBS staff, to the organization itself.
    Staff,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuditEntryDto {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    /// As they were called then; NOALBS for staff.
    pub actor_name: String,
    pub actor: AuditActor,
    /// A stable code, e.g. `client_deleted`.
    pub action: String,
    /// What it was about, e.g. a client's or member's name.
    pub subject: Option<String>,
    /// Extra facts about it (what changed, how many), by action.
    #[schema(value_type = Object)]
    pub details: serde_json::Value,
}

impl From<db::organization_audit::Entry> for AuditEntryDto {
    fn from(entry: db::organization_audit::Entry) -> Self {
        Self {
            id: entry.id,
            created_at: entry.created_at,
            actor_name: entry.actor_name,
            actor: match entry.actor_kind.as_str() {
                "team" => AuditActor::Team,
                "customer" => AuditActor::Customer,
                _ => AuditActor::Staff,
            },
            action: entry.action,
            subject: entry.subject,
            details: entry.details,
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct AuditQuery {
    /// Only entries before this time, for paging back.
    pub before: Option<DateTime<Utc>>,
    /// At most this many (default 50, up to 200).
    pub limit: Option<i64>,
}

/// What the team, the customers and NOALBS changed in the organization, newest
/// first. Admins only.
#[utoipa::path(get, path = "/organizations/{organization_id}/audit", tag = "organizations",
    params(("organization_id" = Uuid, Path), AuditQuery),
    responses((status = OK, body = Vec<AuditEntryDto>), (status = NOT_FOUND, body = ErrorBody)))]
async fn organization_audit(
    State(state): State<ApiState>,
    authorized: OrganizationAuthorized<OrgManage>,
    Query(query): Query<AuditQuery>,
) -> ApiResult<Json<Vec<AuditEntryDto>>> {
    let access = authorized.into_access();
    let limit = query.limit.unwrap_or(DEFAULT_ENTRIES).clamp(1, MAX_ENTRIES);
    let entries =
        db::organization_audit::list(&state.db, access.organization_id, query.before, limit)
            .await?;

    Ok(Json(entries.into_iter().map(Into::into).collect()))
}
