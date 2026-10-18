//! A few seconds of memory for the two lookups every request starts with: whose
//! session this is, and whether that member may do this in the workspace. The
//! dashboard polls (OBS stats, the preview, live status), so the same answers were
//! being read from Postgres many times a second.
//!
//! Nothing is invalidated: an entry expires after [`AUTH_TTL`], per API replica.
//! So signing a session out, disabling a user, removing a member, changing a role
//! or suspending a workspace takes up to that long to reach a request that is
//! already being served from memory. Only answers that let a request through are
//! kept (a missing session or a refusal is asked again at once, so signing in or
//! being let in shows immediately), and only permissions for looking at or running
//! a switcher: billing, ownership, deleting and support access always read the
//! database, and so does NOALBS support, whose grants are logged and revocable.

use std::time::Duration;

use common::ttl_cache::TtlCache;
use db::{
    users::User,
    workspaces::{MemberAccess, WorkspacePermission},
};
use uuid::Uuid;

/// How long a session's user and a member's permission are trusted.
pub const AUTH_TTL: Duration = Duration::from_secs(5);

/// The permissions that may be answered from memory: what the dashboard polls.
const CACHEABLE: [WorkspacePermission; 3] = [
    WorkspacePermission::ACCOUNT_VIEW,
    WorkspacePermission::SWITCHER_VIEW,
    WorkspacePermission::SWITCHER_CONTROL,
];

pub struct AuthCache {
    /// `None` (no such session) is never kept.
    users: TtlCache<Uuid, Option<User>>,
    /// (workspace, user, permission); only a member, or an organization team member, who is allowed is kept.
    members: TtlCache<(Uuid, Uuid, &'static str), MemberAccess>,
}

impl AuthCache {
    pub fn new() -> Self {
        Self {
            users: TtlCache::new(AUTH_TTL),
            members: TtlCache::new(AUTH_TTL),
        }
    }

    /// The user of a session that hasn't expired (see `db::sessions::user_for_active_session`).
    pub async fn user(
        &self,
        pool: &db::DbPool,
        session_id: Uuid,
    ) -> Result<Option<User>, db::Error> {
        self.users
            .get_or_load_if(
                session_id,
                || db::sessions::user_for_active_session(pool, session_id),
                Option::is_some,
            )
            .await
    }

    /// Whether the member has the permission, and whether the workspace is suspended.
    pub async fn member_access(
        &self,
        pool: &db::DbPool,
        workspace_id: Uuid,
        user_id: Uuid,
        permission: WorkspacePermission,
    ) -> Result<MemberAccess, db::Error> {
        let load = || db::workspaces::member_access(pool, workspace_id, user_id, permission);
        if !CACHEABLE.contains(&permission) {
            return load().await;
        }
        self.members
            .get_or_load_if(
                (workspace_id, user_id, permission.as_str()),
                load,
                |access| access.allowed || access.organization_allowed,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_permissions_the_dashboard_polls_are_cached() {
        for permission in [
            WorkspacePermission::ACCOUNT_VIEW,
            WorkspacePermission::SWITCHER_VIEW,
            WorkspacePermission::SWITCHER_CONTROL,
        ] {
            assert!(CACHEABLE.contains(&permission), "{}", permission.as_str());
        }
        for permission in [
            WorkspacePermission::ACCOUNT_MANAGE,
            WorkspacePermission::SWITCHER_MANAGE,
            WorkspacePermission::BILLING_MANAGE,
            WorkspacePermission::WORKSPACE_DELETE,
            WorkspacePermission::WORKSPACE_TRANSFER,
            WorkspacePermission::WORKSPACE_SUPPORT_ACCESS,
        ] {
            assert!(!CACHEABLE.contains(&permission), "{}", permission.as_str());
        }
    }
}
