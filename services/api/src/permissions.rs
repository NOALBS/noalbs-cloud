use axum::extract::{FromRequestParts, MatchedPath, RawPathParams};
use axum::http::request::Parts;
use axum_extra::extract::cookie::PrivateCookieJar;
use db::organizations::OrganizationPermission;
use db::support::SupportLevel;
use db::workspaces::{WorkspacePermission, WorkspaceRoleEnum};
use std::marker::PhantomData;
use uuid::Uuid;

use crate::{app::ApiState, routes::ApiError};

/// The authenticated API user, resolved from a valid private session cookie.
#[derive(Debug, Clone)]
pub struct CurrentUser(pub db::users::User);

impl CurrentUser {
    pub fn id(&self) -> Uuid {
        self.0.id
    }

    pub fn is_staff(&self) -> bool {
        self.0.staff
    }
}

impl FromRequestParts<ApiState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ApiState,
    ) -> std::result::Result<Self, Self::Rejection> {
        let cookies = PrivateCookieJar::from_request_parts(parts, state)
            .await
            .expect("PrivateCookieJar extraction is infallible");
        let session_id = session_id_from_cookie(&cookies)?;
        let user = state
            .auth
            .user(&state.db, session_id)
            .await?
            .ok_or_else(|| ApiError::unauthorized("Invalid or expired session"))?;
        record_on_request("user.id", user.id);

        Ok(Self(user))
    }
}

/// A signed-in staff user, for the admin section. Anyone else gets 403.
#[derive(Debug, Clone)]
pub struct Staff(pub CurrentUser);

impl FromRequestParts<ApiState> for Staff {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ApiState,
    ) -> std::result::Result<Self, Self::Rejection> {
        let user = CurrentUser::from_request_parts(parts, state).await?;
        if !user.is_staff() {
            return Err(ApiError::forbidden("forbidden", "Staff only"));
        }
        Ok(Self(user))
    }
}

/// The signed-in user plus the session they're using, for routes that manage
/// sessions and connections ("this device").
#[derive(Debug, Clone, Copy)]
pub struct CurrentSession {
    pub user_id: Uuid,
    pub session_id: Uuid,
}

impl FromRequestParts<ApiState> for CurrentSession {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ApiState,
    ) -> std::result::Result<Self, Self::Rejection> {
        let cookies = PrivateCookieJar::from_request_parts(parts, state)
            .await
            .expect("PrivateCookieJar extraction is infallible");
        let session_id = session_id_from_cookie(&cookies)?;
        let user_id = db::sessions::user_id_for_active_session(&state.db, session_id)
            .await?
            .ok_or_else(|| ApiError::unauthorized("Invalid or expired session"))?;
        record_on_request("user.id", user_id);

        Ok(Self {
            user_id,
            session_id,
        })
    }
}

/// The static permission a route requires for a workspace resource.
pub trait WorkspacePermissionRequirement {
    const PERMISSION: WorkspacePermission;
}

macro_rules! permission_markers {
    ($($(#[$doc:meta])* $marker:ident => $permission:ident;)*) => {
        $(
            $(#[$doc])*
            pub struct $marker;

            impl WorkspacePermissionRequirement for $marker {
                const PERMISSION: WorkspacePermission = WorkspacePermission::$permission;
            }
        )*
    };
}

permission_markers! {
    /// Requires `account.view` for the workspace in the route path.
    AccountView => ACCOUNT_VIEW;
    /// Requires `account.manage` for the workspace in the route path.
    AccountManage => ACCOUNT_MANAGE;
    /// Requires `switcher.view` for the workspace in the route path.
    SwitcherView => SWITCHER_VIEW;
    /// Requires `switcher.control` for the workspace in the route path.
    SwitcherControl => SWITCHER_CONTROL;
    /// Requires `switcher.manage` for the workspace in the route path.
    SwitcherManage => SWITCHER_MANAGE;
    /// Requires `billing.manage` for the workspace in the route path.
    BillingManage => BILLING_MANAGE;
    /// Requires `workspace.delete` (the owner) for the workspace in the route path.
    WorkspaceDelete => WORKSPACE_DELETE;
    /// Requires `workspace.transfer` (the owner) for the workspace in the route path.
    WorkspaceTransfer => WORKSPACE_TRANSFER;
    /// Requires `workspace.support_access` (owner, admin) for the workspace in the route path.
    SupportAccessManage => WORKSPACE_SUPPORT_ACCESS;
}

/// The static permission a route requires for an organization.
pub trait OrganizationPermissionRequirement {
    const PERMISSION: OrganizationPermission;
}

macro_rules! organization_permission_markers {
    ($($(#[$doc:meta])* $marker:ident => $permission:ident;)*) => {
        $(
            $(#[$doc])*
            pub struct $marker;

            impl OrganizationPermissionRequirement for $marker {
                const PERMISSION: OrganizationPermission = OrganizationPermission::$permission;
            }
        )*
    };
}

organization_permission_markers! {
    /// Requires `org.view` for the organization in the route path.
    OrgView => VIEW;
    /// Requires `org.manage` (create, change and delete clients) for the organization in the route path.
    OrgManage => MANAGE;
    /// Requires `org.members` (the team) for the organization in the route path.
    OrgMembers => MEMBERS;
}

/// An organization from the route path whose team the current user is on with
/// the route's permission. Staff go through the admin routes instead.
pub struct OrganizationAuthorized<P>(pub OrganizationAccess, PhantomData<P>);

impl<P> OrganizationAuthorized<P> {
    pub fn into_access(self) -> OrganizationAccess {
        self.0
    }
}

pub struct OrganizationAccess {
    pub organization_id: Uuid,
    pub user: CurrentUser,
}

impl<P> FromRequestParts<ApiState> for OrganizationAuthorized<P>
where
    P: OrganizationPermissionRequirement + Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ApiState,
    ) -> std::result::Result<Self, Self::Rejection> {
        let organization_id = uuid_path_param(parts, state, "organization_id").await?;
        let user = CurrentUser::from_request_parts(parts, state).await?;

        // Not on the team: the same answer as for one that doesn't exist.
        if db::organizations::member_role(&state.db, organization_id, user.id())
            .await?
            .is_none()
        {
            return Err(ApiError::not_found("Organization not found"));
        }
        if !db::organizations::has_permission(&state.db, organization_id, user.id(), P::PERMISSION)
            .await?
        {
            return Err(ApiError::forbidden(
                "forbidden",
                "You do not have permission to do this in this organization",
            ));
        }

        Ok(Self(
            OrganizationAccess {
                organization_id,
                user,
            },
            PhantomData,
        ))
    }
}

/// A workspace resource for which the route's permission requirement is met.
///
/// `P` is only a compile-time marker. It makes the permission policy visible in
/// the handler signature while the extractor performs the database check.
pub struct WorkspaceAuthorized<P>(pub WorkspaceAccess, PhantomData<P>);

impl<P> WorkspaceAuthorized<P> {
    pub fn into_access(self) -> WorkspaceAccess {
        self.0
    }
}

impl<P> FromRequestParts<ApiState> for WorkspaceAuthorized<P>
where
    P: WorkspacePermissionRequirement + Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ApiState,
    ) -> std::result::Result<Self, Self::Rejection> {
        WorkspaceAccess::extract(parts, state, P::PERMISSION)
            .await
            .map(|access| Self(access, PhantomData))
    }
}

/// The workspace and authenticated user established by a permission extractor.
pub struct WorkspaceAccess {
    pub workspace_id: Uuid,
    pub user: CurrentUser,
    pub via: Via,
    /// The organization that owns this workspace, when it is a client workspace.
    pub organization_name: Option<String>,
}

/// How the user got into the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// A member whose role has the permission.
    Member,
    /// On the team of the organization that owns this client workspace, whose role
    /// there has the permission. The team acts as the owner without being a member.
    Organization,
    /// Staff without that permission as a member: NOALBS support, under a grant
    /// that covers the request (the workspace's own, or a break-glass one).
    /// Every request is logged for the workspace (`db::support`).
    Support {
        grant_id: Uuid,
        level: SupportLevel,
        break_glass: bool,
    },
}

impl WorkspaceAccess {
    async fn extract(
        parts: &mut Parts,
        state: &ApiState,
        permission: WorkspacePermission,
    ) -> std::result::Result<Self, ApiError> {
        let workspace_id = uuid_path_param(parts, state, "workspace_id").await?;
        record_on_request("workspace.id", workspace_id);
        let user = CurrentUser::from_request_parts(parts, state).await?;

        let member = state
            .auth
            .member_access(&state.db, workspace_id, user.id(), permission)
            .await?;
        if member.allowed || member.organization_allowed {
            // Suspended by staff: members can still look, fix billing or
            // leave, but not run or change switchers.
            if [
                WorkspacePermission::SWITCHER_CONTROL,
                WorkspacePermission::SWITCHER_MANAGE,
            ]
            .contains(&permission)
                && let Some(reason) = member.suspended_reason
            {
                // Its organization's suspension is not for its customers to read.
                let message = match (&member.organization_name, member.suspended_by_organization) {
                    (Some(organization), true) if !member.organization_allowed => {
                        format!("This workspace is paused. {organization} manages it: ask them.")
                    }
                    _ => format!("NOALBS suspended this workspace: {reason}"),
                };
                return Err(ApiError::forbidden("workspace_suspended", message));
            }
            return Ok(Self {
                workspace_id,
                user,
                via: if member.allowed {
                    Via::Member
                } else {
                    Via::Organization
                },
                organization_name: member.organization_name,
            });
        }
        if !user.is_staff() {
            return Err(ApiError::forbidden(
                "forbidden",
                "You do not have permission to do this in this workspace",
            ));
        }

        // Nothing was seen or done without a grant, so a refusal isn't logged.
        let grant = db::support::active_grants(&state.db, workspace_id)
            .await?
            .into_iter()
            .find(|grant| support_allows(grant.level, permission))
            .ok_or_else(|| {
                ApiError::forbidden(
                    "support_access_required",
                    "The workspace hasn't allowed NOALBS support to do this",
                )
            })?;
        let staff_name = user_display_name(state, user.id()).await?;
        let route = parts.extensions.get::<MatchedPath>().map_or_else(
            || parts.uri.path().to_owned(),
            |path| path.as_str().to_owned(),
        );
        let switcher_id = uuid_path_param(parts, state, "switcher_id").await.ok();
        db::support::log_access(
            &state.db,
            &db::support::Access {
                workspace_id,
                grant_id: Some(grant.id),
                staff_user_id: user.id(),
                staff_name: &staff_name,
                method: parts.method.as_str(),
                route: &route,
                switcher_id,
            },
        )
        .await?;

        Ok(Self {
            workspace_id,
            user,
            via: Via::Support {
                grant_id: grant.id,
                level: grant.level,
                break_glass: grant.break_glass,
            },
            organization_name: member.organization_name,
        })
    }

    /// Whether this is NOALBS support rather than a member.
    pub fn is_support(&self) -> bool {
        matches!(self.via, Via::Support { .. })
    }

    /// Refuses NOALBS support, for what only the workspace's members may do.
    pub fn members_only(&self) -> std::result::Result<(), ApiError> {
        if self.is_support() {
            return Err(ApiError::forbidden(
                "members_only",
                "Only members of the workspace can do this",
            ));
        }
        Ok(())
    }

    /// Refuses support on routes that hand out a secret or a way to one (raw
    /// stats, the Moblin password, the connect token, a stored stream server
    /// password sent to another address). Stats URLs aren't among them.
    pub fn secrets_allowed(&self) -> std::result::Result<(), ApiError> {
        if self.is_support() {
            return Err(ApiError::forbidden(
                "support_secret",
                "NOALBS support can't see or use the workspace's secrets",
            ));
        }
        Ok(())
    }

    /// Who did it, for the activity feed; support is marked as such.
    pub async fn display_name(&self, state: &ApiState) -> std::result::Result<String, ApiError> {
        let name = user_display_name(state, self.user.id()).await?;
        Ok(match self.via {
            Via::Member => format!("{name} (Web)"),
            Via::Organization => match &self.organization_name {
                Some(organization) => format!("{name} ({organization})"),
                None => format!("{name} (Web)"),
            },
            Via::Support { .. } => format!("NOALBS support ({name})"),
        })
    }
}

/// What a support grant of `level` lets staff do. Billing, members, deleting
/// or handing over the workspace and support access itself are never included.
pub fn support_allows(level: SupportLevel, permission: WorkspacePermission) -> bool {
    let view = [
        WorkspacePermission::ACCOUNT_VIEW,
        WorkspacePermission::SWITCHER_VIEW,
    ];
    let control = [
        WorkspacePermission::SWITCHER_CONTROL,
        WorkspacePermission::SWITCHER_MANAGE,
    ];
    match level {
        SupportLevel::View => view.contains(&permission),
        SupportLevel::Control => view.contains(&permission) || control.contains(&permission),
    }
}

/// A user's display name: their most recently used linked account's.
pub async fn user_display_name(
    state: &ApiState,
    user_id: Uuid,
) -> std::result::Result<String, ApiError> {
    let profiles = db::oauth2_connections::list_profiles_for_user(&state.db, user_id).await?;
    Ok(profiles
        .into_iter()
        .next()
        .map_or_else(|| "Someone".to_owned(), |profile| profile.provider_username))
}

/// Who is acting in a workspace, for rules that depend on the actor's role
/// (such as which roles they may hand out).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// Staff, acting in a workspace they may not be a member of.
    Staff,
    Member(WorkspaceRoleEnum),
    /// The team of the organization that owns a client workspace.
    Organization,
}

impl Actor {
    /// Whether this actor has owner-level say over roles and members.
    pub fn is_owner_or_staff(self) -> bool {
        matches!(
            self,
            Self::Staff | Self::Organization | Self::Member(WorkspaceRoleEnum::Owner)
        )
    }
}

impl WorkspaceAccess {
    /// Whether the user also has `permission` here, beyond what the route required.
    pub async fn can(
        &self,
        state: &ApiState,
        permission: WorkspacePermission,
    ) -> std::result::Result<bool, ApiError> {
        if let Via::Support { level, .. } = self.via
            && support_allows(level, permission)
        {
            return Ok(true);
        }

        let access =
            db::workspaces::member_access(&state.db, self.workspace_id, self.user.id(), permission)
                .await?;
        Ok(access.allowed || access.organization_allowed)
    }

    /// The actor's standing in this workspace.
    pub async fn actor(&self, state: &ApiState) -> std::result::Result<Actor, ApiError> {
        if self.user.is_staff() {
            return Ok(Actor::Staff);
        }
        if self.via == Via::Organization {
            return Ok(Actor::Organization);
        }

        db::workspaces::member_role(&state.db, self.workspace_id, self.user.id())
            .await?
            .map(Actor::Member)
            .ok_or_else(|| {
                ApiError::forbidden("forbidden", "You are not a member of this workspace")
            })
    }
}

/// A switcher in the workspace from the route path, for which the route's
/// permission requirement is met.
///
/// Reads the `workspace_id` and `switcher_id` path parameters and answers 404
/// when the switcher belongs to another workspace.
pub struct SwitcherAuthorized<P>(pub SwitcherAccess, PhantomData<P>);

impl<P> SwitcherAuthorized<P> {
    pub fn into_access(self) -> SwitcherAccess {
        self.0
    }
}

pub struct SwitcherAccess {
    pub workspace: WorkspaceAccess,
    pub switcher: db::switchers::Switcher,
}

impl<P> FromRequestParts<ApiState> for SwitcherAuthorized<P>
where
    P: WorkspacePermissionRequirement + Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ApiState,
    ) -> std::result::Result<Self, Self::Rejection> {
        let workspace = WorkspaceAccess::extract(parts, state, P::PERMISSION).await?;
        let switcher_id = uuid_path_param(parts, state, "switcher_id").await?;
        record_on_request("switcher.id", switcher_id);
        let switcher =
            db::switchers::get_in_workspace(&state.db, workspace.workspace_id, switcher_id)
                .await?
                .ok_or_else(|| ApiError::not_found("Switcher not found"))?;

        Ok(Self(
            SwitcherAccess {
                workspace,
                switcher,
            },
            PhantomData,
        ))
    }
}

/// Reads a UUID path parameter by name, so extractors work on routes with
/// several parameters.
async fn uuid_path_param(
    parts: &mut Parts,
    state: &ApiState,
    name: &str,
) -> std::result::Result<Uuid, ApiError> {
    let params = RawPathParams::from_request_parts(parts, state)
        .await
        .map_err(|_| ApiError::bad_request("invalid_path", "Invalid path parameters"))?;
    let value = params
        .iter()
        .find_map(|(key, value)| (key == name).then_some(value))
        .ok_or_else(|| ApiError::bad_request("invalid_path", format!("Missing {name}")))?;

    Uuid::parse_str(value)
        .map_err(|_| ApiError::bad_request("invalid_path", format!("Invalid {name}")))
}

/// The name of the encrypted cookie containing a `user_sessions.id`.
pub const SESSION_COOKIE_NAME: &str = "id";

fn session_id_from_cookie(cookies: &PrivateCookieJar) -> std::result::Result<Uuid, ApiError> {
    let session = cookies
        .get(SESSION_COOKIE_NAME)
        .ok_or_else(|| ApiError::unauthorized("Missing session cookie"))?;

    Uuid::parse_str(session.value()).map_err(|_| ApiError::unauthorized("Invalid session cookie"))
}

/// Puts who/what a request is about on its span (`request_spans`).
fn record_on_request(field: &'static str, id: Uuid) {
    tracing::Span::current().record(field, tracing::field::display(id));
}

#[cfg(test)]
mod tests {
    use axum_extra::extract::cookie::{Cookie, Key, PrivateCookieJar};
    use uuid::Uuid;

    use db::support::SupportLevel;
    use db::workspaces::WorkspacePermission;

    use super::{
        AccountManage, AccountView, BillingManage, SESSION_COOKIE_NAME, SupportAccessManage,
        SwitcherControl, SwitcherManage, SwitcherView, WorkspacePermissionRequirement,
        session_id_from_cookie, support_allows,
    };

    #[test]
    fn permission_markers_use_the_expected_database_permissions() {
        assert_eq!(AccountView::PERMISSION.as_str(), "account.view");
        assert_eq!(AccountManage::PERMISSION.as_str(), "account.manage");
        assert_eq!(SwitcherView::PERMISSION.as_str(), "switcher.view");
        assert_eq!(SwitcherControl::PERMISSION.as_str(), "switcher.control");
        assert_eq!(SwitcherManage::PERMISSION.as_str(), "switcher.manage");
        assert_eq!(BillingManage::PERMISSION.as_str(), "billing.manage");
        assert_eq!(
            SupportAccessManage::PERMISSION.as_str(),
            "workspace.support_access"
        );
    }

    #[test]
    fn support_grants_cover_switchers_but_never_the_workspace_itself() {
        use SupportLevel::{Control, View};
        use WorkspacePermission as P;

        assert!(support_allows(View, P::ACCOUNT_VIEW));
        assert!(support_allows(View, P::SWITCHER_VIEW));
        assert!(!support_allows(View, P::SWITCHER_CONTROL));
        assert!(!support_allows(View, P::SWITCHER_MANAGE));
        assert!(support_allows(Control, P::SWITCHER_CONTROL));
        assert!(support_allows(Control, P::SWITCHER_MANAGE));
        for level in [View, Control] {
            for never in [
                P::ACCOUNT_MANAGE,
                P::BILLING_MANAGE,
                P::WORKSPACE_DELETE,
                P::WORKSPACE_TRANSFER,
                P::WORKSPACE_SUPPORT_ACCESS,
            ] {
                assert!(!support_allows(level, never));
            }
        }
    }

    #[test]
    fn parses_private_session_cookie() {
        let session_id = Uuid::new_v4();
        let cookies = PrivateCookieJar::new(Key::generate())
            .add(Cookie::new(SESSION_COOKIE_NAME, session_id.to_string()));

        assert_eq!(session_id_from_cookie(&cookies).unwrap(), session_id);
    }

    #[test]
    fn rejects_missing_or_malformed_session_cookie() {
        assert!(session_id_from_cookie(&PrivateCookieJar::new(Key::generate())).is_err());

        let cookies = PrivateCookieJar::new(Key::generate())
            .add(Cookie::new(SESSION_COOKIE_NAME, "not-a-uuid"));
        assert!(session_id_from_cookie(&cookies).is_err());
    }

    #[test]
    fn organization_markers_use_the_expected_database_permissions() {
        use super::{OrgManage, OrgMembers, OrgView, OrganizationPermissionRequirement};

        assert_eq!(OrgView::PERMISSION.as_str(), "org.view");
        assert_eq!(OrgManage::PERMISSION.as_str(), "org.manage");
        assert_eq!(OrgMembers::PERMISSION.as_str(), "org.members");
    }

    #[test]
    fn an_organization_has_owner_level_say_over_roles() {
        use super::Actor;

        // What it may do beyond that comes from `organization_role_permissions`
        // (no billing, no transfer: see the db tests).
        assert!(Actor::Organization.is_owner_or_staff());
    }
}
