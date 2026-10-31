import { plural, t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { ChevronLeft } from "lucide-react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { adminUserQuery, queryKeys } from "@/api/queries";
import type { AdminUser, AdminUserDetail } from "@/api/types";
import { Stat } from "@/components/admin/list-controls";
import { ReasonDialog } from "@/components/admin/reason-dialog";
import { ConfirmButton } from "@/components/billing/confirm-button";
import { Page, Section } from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { UserAvatar } from "@/components/user-avatar";
import { errorMessage } from "@/lib/errors";
import { formatDate, formatRelative } from "@/lib/time";

export const Route = createFileRoute("/_app/admin/users/$user")({
  component: UserPage,
});

function UserPage() {
  const { user: userId } = Route.useParams();
  const detail = useQuery(adminUserQuery(userId));
  const { me } = Route.useRouteContext();
  const queryClient = useQueryClient();
  const staff = useMutation({
    mutationFn: (value: boolean) =>
      unwrap(
        api.PUT("/admin/users/{user_id}/staff", {
          params: { path: { user_id: userId } },
          body: { staff: value },
        }),
      ),
    onSuccess: async (result) => {
      queryClient.setQueryData(queryKeys.adminUser(userId), result);
      await queryClient.invalidateQueries({ queryKey: queryKeys.admin });
      toast.success(result.user.staff ? t`Staff access granted` : t`Staff access removed`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const back = (
    <Link to="/admin/users" className="flex items-center gap-1 text-sm font-medium text-primary">
      <ChevronLeft className="size-4" />
      <Trans>Users</Trans>
    </Link>
  );
  const data = detail.data;
  if (!data) {
    return (
      <Page title={t`User`} description={back} className="max-w-5xl">
        {detail.isError ? (
          <p className="text-sm">{errorMessage(detail.error)}</p>
        ) : (
          <Skeleton className="h-40" />
        )}
      </Page>
    );
  }
  const user = data.user;
  const isMe = user.id === me.id;

  return (
    <Page
      title={user.username ?? t`User`}
      description={back}
      className="max-w-5xl"
      actions={
        isMe && user.staff ? (
          <span className="text-xs text-muted-foreground">
            <Trans>You</Trans>
          </span>
        ) : (
          <ConfirmButton
            variant={user.staff ? "outline" : "default"}
            disabled={staff.isPending}
            title={user.staff ? t`Remove staff access?` : t`Make staff?`}
            confirmLabel={user.staff ? t`Remove staff` : t`Make staff`}
            onConfirm={() => staff.mutate(!user.staff)}
            description={
              <span>
                {user.staff
                  ? t`They lose the admin section and access to workspaces they aren't a member of.`
                  : t`Staff can open every workspace, change plans and prices, and manage other staff.`}
              </span>
            }
          >
            {user.staff ? t`Remove staff` : t`Make staff`}
          </ConfirmButton>
        )
      }
    >
      <div className="flex items-center gap-3">
        <UserAvatar name={user.username} avatarUrl={user.avatar_url} className="size-12" />
        <div className="flex flex-col gap-0.5">
          <span className="flex items-center gap-2 font-semibold">
            {user.username ?? t`No sign-in`}
            {user.staff && (
              <Badge variant="secondary">
                <Trans>Staff</Trans>
              </Badge>
            )}
            {user.disabled_at && (
              <Badge variant="destructive">
                <Trans>Disabled</Trans>
              </Badge>
            )}
          </span>
          <code className="text-xs text-muted-foreground">{user.id}</code>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-3 md:grid-cols-3">
        <Stat label={t`Joined`} value={formatDate(user.created_at)} />
        <Stat
          label={t`Last active`}
          value={user.last_active_at ? formatRelative(user.last_active_at) : "Never"}
        />
        <Stat label={t`Workspaces`} value={user.workspaces} />
      </div>

      {!isMe && <AccountActions userId={userId} user={user} />}

      <Section title={t`Sign-ins`}>
        <ul className="flex flex-col pb-2">
          {data.connections.map((connection) => (
            <li
              key={`${connection.provider}-${connection.username}`}
              className="flex items-center gap-3 px-5 py-2.5"
            >
              <UserAvatar name={connection.username} avatarUrl={connection.avatar_url} />
              <span className="min-w-0 flex-1 truncate text-sm font-medium">
                {connection.username}
              </span>
              <Badge variant="outline">{connection.provider.toLowerCase()}</Badge>
              <span className="hidden text-xs text-muted-foreground sm:inline">
                <Trans>since {formatDate(connection.created_at)}</Trans>
              </span>
            </li>
          ))}
        </ul>
      </Section>

      <Section title={t`Workspaces`}>
        {data.workspaces.length === 0 ? (
          <p className="px-5 pb-5 text-sm text-muted-foreground">
            <Trans>Not a member of any.</Trans>
          </p>
        ) : (
          <ul className="flex flex-col pb-2">
            {data.workspaces.map((workspace) => (
              <li key={workspace.id}>
                <Link
                  to="/admin/workspaces/$workspace"
                  params={{ workspace: workspace.id }}
                  className="flex items-center gap-3 px-5 py-2.5 hover:bg-surface-2"
                >
                  <span className="min-w-0 flex-1 truncate text-sm font-medium">
                    {workspace.name}
                  </span>
                  {workspace.role && <Badge variant="outline">{workspace.role}</Badge>}
                  <Badge variant="secondary" className="uppercase">
                    {workspace.plan_code}
                  </Badge>
                </Link>
              </li>
            ))}
          </ul>
        )}
      </Section>
    </Page>
  );
}

/** Signing someone out everywhere, and disabling or enabling the account. */
function AccountActions({ userId, user }: { userId: string; user: AdminUser }) {
  const queryClient = useQueryClient();
  const refresh = async (result: AdminUserDetail) => {
    queryClient.setQueryData(queryKeys.adminUser(userId), result);
    await queryClient.invalidateQueries({ queryKey: queryKeys.admin });
  };
  const signOut = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE("/admin/users/{user_id}/sessions", {
          params: { path: { user_id: userId } },
        }),
      ),
    onSuccess: (result) =>
      toast.success(
        plural(result.sessions, {
          one: "Signed out of # session",
          other: "Signed out of # sessions",
        }),
      ),
    onError: (error) => toast.error(errorMessage(error)),
  });
  const enable = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE("/admin/users/{user_id}/disabled", {
          params: { path: { user_id: userId } },
        }),
      ),
    onSuccess: async (result) => {
      await refresh(result);
      toast.success(t`They can sign in again`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section title={t`Account`} description={t`Recorded in the staff audit log.`}>
      <div className="flex flex-col gap-3 px-5 pb-5 text-sm">
        {user.disabled_at && (
          <p>
            <strong>
              <Trans>Disabled</Trans>
            </strong>{" "}
            {formatRelative(user.disabled_at)}: {user.disabled_reason}
          </p>
        )}
        <div className="flex flex-wrap gap-2">
          <ConfirmButton
            variant="outline"
            disabled={signOut.isPending}
            title={t`Sign them out everywhere?`}
            confirmLabel={t`Sign out everywhere`}
            onConfirm={() => signOut.mutate()}
            description={
              <span>
                <Trans>Every browser they're signed in on. They can sign in again.</Trans>
              </span>
            }
          >
            <Trans>Sign out everywhere</Trans>
          </ConfirmButton>
          {user.disabled_at ? (
            <ConfirmButton
              variant="outline"
              disabled={enable.isPending}
              title={t`Enable the account?`}
              confirmLabel={t`Enable`}
              onConfirm={() => enable.mutate()}
              description={
                <span>
                  <Trans>They can sign in again.</Trans>
                </span>
              }
            >
              <Trans>Enable account</Trans>
            </ConfirmButton>
          ) : (
            !user.staff && (
              <ReasonDialog
                title={t`Disable this account?`}
                description={t`They're signed out everywhere and can't sign in until enabled. Their workspaces keep running; suspend those separately.`}
                confirmLabel={t`Disable`}
                onConfirm={async (reason) => {
                  const result = await unwrap(
                    api.PUT("/admin/users/{user_id}/disabled", {
                      params: { path: { user_id: userId } },
                      body: { reason },
                    }),
                  );
                  await refresh(result);
                  toast.success(t`Account disabled`);
                }}
              >
                <Button
                  variant="outline"
                  className="border-destructive/50 text-destructive-foreground"
                >
                  <Trans>Disable account…</Trans>
                </Button>
              </ReasonDialog>
            )
          )}
        </div>
      </div>
    </Section>
  );
}
