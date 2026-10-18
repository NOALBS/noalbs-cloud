use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{AccountManage, AccountView, CurrentUser, WorkspaceAuthorized, WorkspaceDelete},
    routes::{
        ApiError, ApiResult, ErrorBody,
        organization_audit::{record, record_in_client},
        organizations::OrganizationRole,
        support::{self, SupportGrantDto},
    },
    validate,
};

/// How many workspaces one user may own (staff: any number), so nobody
/// multiplies the Free plan.
pub const MAX_OWNED_WORKSPACES: i64 = 5;

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_workspaces, create_workspace))
        .routes(routes!(slug_available))
        .routes(routes!(get_workspace_by_slug))
        .routes(routes!(get_workspace, update_workspace, delete_workspace))
}

/// A role a member has in a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRole {
    Operator,
    Admin,
    Owner,
}

impl From<db::workspaces::WorkspaceRoleEnum> for WorkspaceRole {
    fn from(role: db::workspaces::WorkspaceRoleEnum) -> Self {
        use db::workspaces::WorkspaceRoleEnum as Db;
        match role {
            Db::Operator => Self::Operator,
            Db::Admin => Self::Admin,
            Db::Owner => Self::Owner,
        }
    }
}

impl From<WorkspaceRole> for db::workspaces::WorkspaceRoleEnum {
    fn from(role: WorkspaceRole) -> Self {
        match role {
            WorkspaceRole::Operator => Self::Operator,
            WorkspaceRole::Admin => Self::Admin,
            WorkspaceRole::Owner => Self::Owner,
        }
    }
}

/// Why and since when NOALBS suspended a workspace.
#[derive(Debug, Serialize, ToSchema)]
pub struct SuspensionDto {
    pub since: chrono::DateTime<chrono::Utc>,
    pub reason: String,
}

/// A workspace and the current user's role in it.
#[derive(Debug, Serialize, ToSchema)]
pub struct WorkspaceDto {
    pub id: Uuid,
    pub name: String,
    pub url_slug: String,
    pub plan_code: String,
    /// The current user's role; `null` for staff who aren't a member.
    pub role: Option<WorkspaceRole>,
    pub switcher_count: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// For staff who aren't a member: what NOALBS support may do here now
    /// (`null` = nothing, so the dashboard asks for permission). Always `null`
    /// for members.
    pub support_access: Option<SupportGrantDto>,
    /// Suspended by NOALBS: switchers paused and chat quiet; `null` otherwise.
    pub suspension: Option<SuspensionDto>,
    /// For a client workspace: the organization that owns, manages and pays for it
    /// (its team can see everything here). `null` for an ordinary workspace.
    pub managed_by: Option<ManagedByDto>,
    /// The current user's role in that organization, when they are on its team.
    pub organization_role: Option<OrganizationRole>,
    /// A client workspace's switcher allocation (`null` = only the organization's pool).
    pub switcher_allocation: Option<i32>,
}

/// The organization that manages a client workspace.
#[derive(Debug, Serialize, ToSchema)]
pub struct ManagedByDto {
    pub id: Uuid,
    pub name: String,
    pub url_slug: String,
}

impl From<db::workspaces::WorkspaceSummary> for WorkspaceDto {
    fn from(workspace: db::workspaces::WorkspaceSummary) -> Self {
        Self {
            id: workspace.id,
            name: workspace.name,
            url_slug: workspace.url_slug,
            plan_code: workspace.plan_code,
            role: workspace.role.map(Into::into),
            switcher_count: workspace.switcher_count,
            created_at: workspace.created_at,
            support_access: None,
            // An organization's suspension is between it and NOALBS: its customers
            // see that the workspace is paused, not why.
            suspension: workspace.suspended_at.map(|since| SuspensionDto {
                since,
                reason: if workspace.suspended_by_organization
                    && workspace.organization_role.is_none()
                    && workspace.role.is_some()
                {
                    String::new()
                } else {
                    workspace.suspended_reason.unwrap_or_default()
                },
            }),
            managed_by: match (
                workspace.organization_id,
                workspace.organization_name,
                workspace.organization_slug,
            ) {
                (Some(id), Some(name), Some(url_slug)) => Some(ManagedByDto { id, name, url_slug }),
                _ => None,
            },
            organization_role: workspace.organization_role.map(Into::into),
            switcher_allocation: workspace.switcher_allocation,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateWorkspace {
    pub name: String,
    pub url_slug: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateWorkspace {
    pub name: String,
}

async fn summary(state: &ApiState, workspace_id: Uuid, user_id: Uuid) -> ApiResult<WorkspaceDto> {
    db::workspaces::get_summary(&state.db, workspace_id, user_id)
        .await?
        .map(Into::into)
        .ok_or_else(|| ApiError::not_found("Workspace not found"))
}

/// List the workspaces the current user is a member of (staff see all).
#[utoipa::path(get, path = "/workspaces", tag = "workspaces",
    responses((status = OK, body = Vec<WorkspaceDto>), (status = UNAUTHORIZED, body = ErrorBody)))]
async fn list_workspaces(
    State(state): State<ApiState>,
    current_user: CurrentUser,
) -> ApiResult<Json<Vec<WorkspaceDto>>> {
    let workspaces = db::workspaces::list_summaries(&state.db, current_user.id()).await?;

    Ok(Json(workspaces.into_iter().map(Into::into).collect()))
}

/// A workspace by its URL slug, for the dashboard's `/{slug}` pages: a member
/// gets it with their role; staff get any workspace (role `null`) with what
/// support may do there (`support_access`).
#[utoipa::path(get, path = "/workspaces/by-slug/{url_slug}", tag = "workspaces",
    params(("url_slug" = String, Path)),
    responses((status = OK, body = WorkspaceDto), (status = NOT_FOUND, body = ErrorBody)))]
async fn get_workspace_by_slug(
    State(state): State<ApiState>,
    current_user: CurrentUser,
    Path(url_slug): Path<String>,
) -> ApiResult<Json<WorkspaceDto>> {
    let workspace =
        db::workspaces::get_summary_by_slug(&state.db, &url_slug, current_user.id()).await?;
    match workspace {
        // Not a member: only the organization's team (for a client workspace) and
        // staff may look, and nobody else learns it exists.
        Some(workspace) if workspace.role.is_some() || workspace.organization_role.is_some() => {
            Ok(Json(workspace.into()))
        }
        Some(workspace) if current_user.is_staff() => {
            let support_access = support::grant_in_force(&state, workspace.id).await?;
            Ok(Json(WorkspaceDto {
                support_access,
                ..workspace.into()
            }))
        }
        _ => Err(ApiError::not_found("Workspace not found")),
    }
}

/// Create a workspace owned by the current user.
#[utoipa::path(post, path = "/workspaces", tag = "workspaces",
    request_body = CreateWorkspace,
    responses(
        (status = CREATED, body = WorkspaceDto),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = FORBIDDEN, description = "`workspace_limit`: you own the most workspaces allowed", body = ErrorBody),
        (status = CONFLICT, description = "`slug_taken`", body = ErrorBody),
    ))]
async fn create_workspace(
    State(state): State<ApiState>,
    current_user: CurrentUser,
    Json(request): Json<CreateWorkspace>,
) -> ApiResult<(StatusCode, Json<WorkspaceDto>)> {
    let name = validate::name(&request.name)?;
    validate::url_slug(&request.url_slug)?;

    let workspace = if current_user.is_staff() {
        db::workspaces::create(&state.db, current_user.id(), name, &request.url_slug)
            .await
            .map_err(slug_conflict)?
    } else {
        db::workspaces::create_within_limit(
            &state.db,
            current_user.id(),
            name,
            &request.url_slug,
            MAX_OWNED_WORKSPACES,
        )
        .await
        .map_err(slug_conflict)?
        .ok_or_else(|| {
            ApiError::forbidden(
                "workspace_limit",
                format!("You can own at most {MAX_OWNED_WORKSPACES} workspaces"),
            )
        })?
    };
    db::users::set_last_workspace(&state.db, current_user.id(), workspace.id).await?;

    Ok((
        StatusCode::CREATED,
        Json(summary(&state, workspace.id, current_user.id()).await?),
    ))
}

fn slug_conflict(error: db::Error) -> ApiError {
    match error.constraint() {
        Some("workspaces_url_slug_key") => {
            ApiError::conflict("slug_taken", "This URL is already taken")
        }
        _ => error.into(),
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct SlugQuery {
    pub slug: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SlugAvailability {
    pub available: bool,
    /// Why the slug can't be used, when it can't.
    pub message: Option<String>,
}

/// Check whether a workspace URL can be used.
#[utoipa::path(get, path = "/workspaces/slug-available", tag = "workspaces",
    params(SlugQuery),
    responses((status = OK, body = SlugAvailability)))]
async fn slug_available(
    State(state): State<ApiState>,
    _current_user: CurrentUser,
    Query(query): Query<SlugQuery>,
) -> ApiResult<Json<SlugAvailability>> {
    if let Err(error) = validate::url_slug(&query.slug) {
        return Ok(Json(SlugAvailability {
            available: false,
            message: Some(error.message().to_owned()),
        }));
    }

    let taken = db::workspaces::slug_exists(&state.db, &query.slug).await?;

    Ok(Json(SlugAvailability {
        available: !taken,
        message: taken.then(|| "This URL is already taken".to_owned()),
    }))
}

/// Get a workspace.
#[utoipa::path(get, path = "/workspaces/{workspace_id}", tag = "workspaces",
    params(("workspace_id" = Uuid, Path)),
    responses((status = OK, body = WorkspaceDto), (status = FORBIDDEN, body = ErrorBody)))]
async fn get_workspace(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<AccountView>,
) -> ApiResult<Json<WorkspaceDto>> {
    let access = authorized.into_access();

    Ok(Json(
        summary(&state, access.workspace_id, access.user.id()).await?,
    ))
}

/// Rename a workspace.
#[utoipa::path(patch, path = "/workspaces/{workspace_id}", tag = "workspaces",
    params(("workspace_id" = Uuid, Path)),
    request_body = UpdateWorkspace,
    responses(
        (status = OK, body = WorkspaceDto),
        (status = FORBIDDEN, body = ErrorBody),
    ))]
async fn update_workspace(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<AccountManage>,
    Json(request): Json<UpdateWorkspace>,
) -> ApiResult<Json<WorkspaceDto>> {
    let access = authorized.into_access();
    let name = validate::name(&request.name)?;
    let previous_name = db::workspaces::get_by_id(&state.db, access.workspace_id)
        .await?
        .map(|workspace| workspace.name);

    let workspace = db::workspaces::rename(&state.db, access.workspace_id, name)
        .await?
        .ok_or_else(|| ApiError::not_found("Workspace not found"))?;
    // The customer's name is on its invoices. Best effort: the rename stands either way.
    if let (Some(billing), Some(customer)) = (&state.billing, &workspace.stripe_customer_id)
        && let Err(error) = billing.stripe.set_customer_name(customer, name).await
    {
        tracing::warn!(%error, workspace_id = %workspace.id, "Couldn't rename the Stripe customer");
    }

    record_in_client(
        &state,
        &access,
        "client_renamed",
        Some(name),
        serde_json::json!({ "previous_name": previous_name }),
    )
    .await?;

    Ok(Json(
        summary(&state, access.workspace_id, access.user.id()).await?,
    ))
}

/// Delete a workspace and everything in it: its switchers and their history,
/// members and invites. A subscription ends at once, without a refund. Owner only.
#[utoipa::path(delete, path = "/workspaces/{workspace_id}", tag = "workspaces",
    params(("workspace_id" = Uuid, Path)),
    responses(
        (status = NO_CONTENT, description = "Deleted"),
        (status = FORBIDDEN, body = ErrorBody),
        (status = BAD_GATEWAY, description = "`stripe_error`: nothing was deleted", body = ErrorBody),
    ))]
async fn delete_workspace(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<WorkspaceDelete>,
) -> ApiResult<StatusCode> {
    let access = authorized.into_access();
    let workspace = db::workspaces::get_by_id(&state.db, access.workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Workspace not found"))?;
    crate::privacy::delete_workspace(
        &state,
        workspace.id,
        workspace.stripe_customer_id.as_deref(),
    )
    .await?;
    // A client workspace's organization keeps a line about it (the workspace is gone).
    if let Some(organization_id) = workspace.organization_id
        && !access.is_support()
    {
        record(
            &state,
            organization_id,
            access.user.id(),
            db::organization_audit::ActorKind::Team,
            "client_deleted",
            Some(&workspace.name),
            serde_json::json!({ "url_slug": workspace.url_slug }),
        )
        .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
