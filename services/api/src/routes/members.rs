use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use db::workspaces::WorkspaceRoleEnum;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

use crate::{
    app::ApiState,
    permissions::{
        AccountManage, AccountView, Actor, WorkspaceAuthorized, WorkspaceTransfer,
        user_display_name,
    },
    routes::{
        ApiError, ApiResult, ErrorBody, organization_audit::record_in_client,
        workspaces::WorkspaceRole,
    },
};

pub fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_members))
        .routes(routes!(update_member, remove_member))
        .routes(routes!(transfer_ownership))
}

/// A workspace member and the profile of their most recently used login.
#[derive(Debug, Serialize, ToSchema)]
pub struct MemberDto {
    pub user_id: Uuid,
    pub role: WorkspaceRole,
    pub username: Option<String>,
    pub avatar_url: Option<String>,
    /// The login the profile comes from, e.g. `TWITCH`.
    pub provider: Option<String>,
    pub joined_at: chrono::DateTime<chrono::Utc>,
}

impl From<db::workspaces::MemberProfile> for MemberDto {
    fn from(member: db::workspaces::MemberProfile) -> Self {
        Self {
            user_id: member.user_id,
            role: member.role.into(),
            username: member.username,
            avatar_url: member.avatar_url,
            provider: member.provider,
            joined_at: member.created_at,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateMember {
    pub role: WorkspaceRole,
}

/// Checks that `actor` may change a member from `current` to `new`.
///
/// Nobody can make someone owner or change the owner. Only the owner (or staff)
/// can hand out or take away the admin role.
pub fn check_role_change(
    actor: Actor,
    current: WorkspaceRoleEnum,
    new: WorkspaceRoleEnum,
) -> ApiResult<()> {
    if new == WorkspaceRoleEnum::Owner {
        return Err(ApiError::bad_request(
            "invalid_role",
            "A workspace has exactly one owner",
        ));
    }
    if current == WorkspaceRoleEnum::Owner {
        return Err(ApiError::forbidden(
            "owner_role",
            "The owner's role can't be changed",
        ));
    }
    let touches_admin = current == WorkspaceRoleEnum::Admin || new == WorkspaceRoleEnum::Admin;
    if touches_admin && !actor.is_owner_or_staff() {
        return Err(ApiError::forbidden(
            "forbidden",
            "Only the owner can change who is an admin",
        ));
    }

    Ok(())
}

/// Checks that `actor` may remove a member with `target` role. Anyone may
/// leave, except the owner; admins may remove operators; the owner (or staff)
/// may remove anyone but themselves.
pub fn check_removal(
    actor: Actor,
    removing_self: bool,
    target: WorkspaceRoleEnum,
) -> ApiResult<()> {
    if target == WorkspaceRoleEnum::Owner {
        return Err(ApiError::forbidden(
            "owner_role",
            "The owner can't leave or be removed. Transfer ownership to an admin first.",
        ));
    }
    if removing_self || actor.is_owner_or_staff() {
        return Ok(());
    }
    if actor == Actor::Member(WorkspaceRoleEnum::Admin) && target == WorkspaceRoleEnum::Operator {
        return Ok(());
    }

    Err(ApiError::forbidden(
        "forbidden",
        "You can't remove this member",
    ))
}

/// List the members of a workspace.
#[utoipa::path(get, path = "/workspaces/{workspace_id}/members", tag = "members",
    params(("workspace_id" = Uuid, Path)),
    responses((status = OK, body = Vec<MemberDto>), (status = FORBIDDEN, body = ErrorBody)))]
async fn list_members(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<AccountView>,
) -> ApiResult<Json<Vec<MemberDto>>> {
    let access = authorized.into_access();
    let members = db::workspaces::list_member_profiles(&state.db, access.workspace_id).await?;

    Ok(Json(members.into_iter().map(Into::into).collect()))
}

async fn target_role(
    state: &ApiState,
    workspace_id: Uuid,
    user_id: Uuid,
) -> ApiResult<WorkspaceRoleEnum> {
    db::workspaces::member_role(&state.db, workspace_id, user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Member not found"))
}

/// Change a member's role.
#[utoipa::path(patch, path = "/workspaces/{workspace_id}/members/{user_id}", tag = "members",
    params(("workspace_id" = Uuid, Path), ("user_id" = Uuid, Path)),
    request_body = UpdateMember,
    responses(
        (status = NO_CONTENT, description = "Role changed"),
        (status = FORBIDDEN, body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ))]
async fn update_member(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<AccountManage>,
    Path(path): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateMember>,
) -> ApiResult<StatusCode> {
    let (_, user_id) = path;
    let access = authorized.into_access();
    let actor = access.actor(&state).await?;
    let current = target_role(&state, access.workspace_id, user_id).await?;
    let new = request.role.into();
    check_role_change(actor, current, new)?;

    db::workspaces::update_member_role(&state.db, access.workspace_id, user_id, new)
        .await?
        .ok_or_else(|| ApiError::not_found("Member not found"))?;
    let name = user_display_name(&state, user_id).await?;
    record_in_client(
        &state,
        &access,
        "customer_role_changed",
        Some(&name),
        serde_json::json!({ "role": request.role }),
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Remove a member, or leave the workspace when `user_id` is yourself.
#[utoipa::path(delete, path = "/workspaces/{workspace_id}/members/{user_id}", tag = "members",
    params(("workspace_id" = Uuid, Path), ("user_id" = Uuid, Path)),
    responses(
        (status = NO_CONTENT, description = "Removed"),
        (status = FORBIDDEN, body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ))]
async fn remove_member(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<AccountView>,
    Path(path): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    let (_, user_id) = path;
    let access = authorized.into_access();
    let actor = access.actor(&state).await?;
    let target = target_role(&state, access.workspace_id, user_id).await?;
    check_removal(actor, user_id == access.user.id(), target)?;

    let name = user_display_name(&state, user_id).await?;
    db::workspaces::remove_member(&state.db, access.workspace_id, user_id).await?;
    let action = if user_id == access.user.id() {
        "customer_left"
    } else {
        "customer_removed"
    };
    record_in_client(&state, &access, action, Some(&name), serde_json::json!({})).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct TransferOwnership {
    /// The member who becomes the owner.
    pub user_id: Uuid,
}

/// Checks that `actor` may make a member with `target` role the owner. The
/// owner hands over to an admin, and not while a payment is past due (the new
/// owner would inherit a failing invoice with no card to pay it). Staff may
/// pick any member either way, e.g. when the owner can't be reached.
pub fn check_transfer(actor: Actor, target: WorkspaceRoleEnum, past_due: bool) -> ApiResult<()> {
    if target == WorkspaceRoleEnum::Owner {
        return Err(ApiError::bad_request(
            "already_owner",
            "This member already owns the workspace",
        ));
    }
    if actor == Actor::Staff {
        return Ok(());
    }
    if target != WorkspaceRoleEnum::Admin {
        return Err(ApiError::bad_request(
            "not_admin",
            "Only an admin can become the owner. Make them an admin first.",
        ));
    }
    if past_due {
        return Err(ApiError::conflict(
            "payment_past_due",
            "A payment for this workspace failed. Update the card under Manage billing before handing it over.",
        ));
    }
    Ok(())
}

/// Make another member the owner; the current owner becomes an admin. With
/// Stripe billing, the workspace's cards, billing email, address and tax IDs are
/// removed first (a subscription keeps running until its period ends; the new
/// owner adds a card to renew it), and if that fails nothing changes.
#[utoipa::path(post, path = "/workspaces/{workspace_id}/transfer", tag = "members",
    params(("workspace_id" = Uuid, Path)),
    request_body = TransferOwnership,
    responses(
        (status = NO_CONTENT, description = "Transferred"),
        (status = BAD_REQUEST, description = "`already_owner` or `not_admin`", body = ErrorBody),
        (status = FORBIDDEN, body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
        (status = CONFLICT, description = "`payment_past_due`", body = ErrorBody),
        (status = BAD_GATEWAY, description = "`stripe_error`: nothing changed", body = ErrorBody),
    ))]
async fn transfer_ownership(
    State(state): State<ApiState>,
    authorized: WorkspaceAuthorized<WorkspaceTransfer>,
    Json(request): Json<TransferOwnership>,
) -> ApiResult<StatusCode> {
    let access = authorized.into_access();
    let actor = access.actor(&state).await?;
    let workspace = db::workspaces::get_by_id(&state.db, access.workspace_id)
        .await?
        .ok_or_else(|| ApiError::not_found("Workspace not found"))?;
    let target = target_role(&state, workspace.id, request.user_id).await?;
    let live = db::billing::live_subscription(&state.db, workspace.id).await?;
    let past_due = live
        .as_ref()
        .is_some_and(|subscription| subscription.status == "past_due");
    check_transfer(actor, target, past_due)?;

    if let Some(customer) = &workspace.stripe_customer_id {
        let Some(billing) = &state.billing else {
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "billing_disabled",
                "This workspace has billing with Stripe, which isn't set up on this server, so its billing details can't be removed.",
            ));
        };
        let subscription = live
            .as_ref()
            .map(|subscription| subscription.stripe_subscription_id.as_str());
        billing
            .stripe
            .remove_billing_details(customer, &workspace.name, subscription)
            .await
            .map_err(|error| {
                tracing::warn!(%error, workspace_id = %workspace.id, "Couldn't remove billing details for a transfer");
                ApiError::new(
                    StatusCode::BAD_GATEWAY,
                    "stripe_error",
                    "Couldn't remove the billing details with the payment provider, so nothing changed. Try again in a moment.",
                )
            })?;
    }

    let transferred = db::workspaces::transfer_ownership(
        &state.db,
        workspace.id,
        request.user_id,
        workspace.stripe_customer_id.is_some(),
    )
    .await?;
    if !transferred {
        return Err(ApiError::not_found("Member not found"));
    }
    tracing::info!(workspace_id = %workspace.id, new_owner = %request.user_id, by = %access.user.id(), "Transferred a workspace");

    Ok(StatusCode::NO_CONTENT)
}
#[cfg(test)]
mod tests {
    use super::{check_removal, check_role_change, check_transfer};
    use crate::permissions::Actor;
    use db::workspaces::WorkspaceRoleEnum::{Admin, Operator, Owner};

    const OWNER: Actor = Actor::Member(Owner);
    const ADMIN: Actor = Actor::Member(Admin);

    #[test]
    fn nobody_can_create_or_change_the_owner() {
        assert!(check_role_change(Actor::Staff, Operator, Owner).is_err());
        assert!(check_role_change(OWNER, Admin, Owner).is_err());
        assert!(check_role_change(Actor::Staff, Owner, Admin).is_err());
    }

    #[test]
    fn only_the_owner_manages_admins() {
        assert!(check_role_change(OWNER, Operator, Admin).is_ok());
        assert!(check_role_change(OWNER, Admin, Operator).is_ok());
        assert!(check_role_change(Actor::Staff, Operator, Admin).is_ok());
        assert!(check_role_change(ADMIN, Operator, Admin).is_err());
        assert!(check_role_change(ADMIN, Admin, Operator).is_err());
        assert!(check_role_change(ADMIN, Operator, Operator).is_ok());
    }

    #[test]
    fn anyone_but_the_owner_can_leave() {
        assert!(check_removal(Actor::Member(Operator), true, Operator).is_ok());
        assert!(check_removal(ADMIN, true, Admin).is_ok());
        assert!(check_removal(OWNER, true, Owner).is_err());
    }

    #[test]
    fn removing_others_depends_on_rank() {
        assert!(check_removal(OWNER, false, Admin).is_ok());
        assert!(check_removal(ADMIN, false, Operator).is_ok());
        assert!(check_removal(ADMIN, false, Admin).is_err());
        assert!(check_removal(Actor::Member(Operator), false, Operator).is_err());
        assert!(check_removal(Actor::Staff, false, Owner).is_err());
    }

    #[test]
    fn the_owner_hands_over_to_an_admin() {
        assert!(check_transfer(OWNER, Admin, false).is_ok());
        assert!(check_transfer(OWNER, Operator, false).is_err());
        assert!(check_transfer(OWNER, Owner, false).is_err());
        // Not with a failed payment the new owner would inherit.
        assert!(check_transfer(OWNER, Admin, true).is_err());
    }

    #[test]
    fn staff_may_pick_any_member() {
        assert!(check_transfer(Actor::Staff, Operator, true).is_ok());
        assert!(check_transfer(Actor::Staff, Admin, false).is_ok());
        assert!(check_transfer(Actor::Staff, Owner, false).is_err());
    }

    #[test]
    fn an_organization_runs_a_client_like_its_owner() {
        // It hands out and takes away admin (the customers) and removes anyone.
        assert!(check_role_change(Actor::Organization, Operator, Admin).is_ok());
        assert!(check_role_change(Actor::Organization, Admin, Operator).is_ok());
        assert!(check_removal(Actor::Organization, false, Admin).is_ok());
        // Nobody can still make the organization's client workspace someone's.
        assert!(check_role_change(Actor::Organization, Operator, Owner).is_err());
    }

    #[test]
    fn a_customer_admin_cannot_touch_other_admins() {
        // The customer joins as an admin: they run the workspace, not who else is an admin.
        assert!(check_role_change(ADMIN, Operator, Admin).is_err());
        assert!(check_removal(ADMIN, false, Admin).is_err());
        assert!(check_removal(ADMIN, true, Admin).is_ok());
    }
}
