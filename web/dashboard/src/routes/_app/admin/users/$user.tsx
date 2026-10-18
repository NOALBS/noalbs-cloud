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
      toast.success(result.user.staff ? "Staff access granted" : "Staff access removed");
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const back = (
    <Link to="/admin/users" className="flex items-center gap-1 text-sm font-medium text-primary">
      <ChevronLeft className="size-4" />
      Users
    </Link>
  );
  const data = detail.data;
  if (!data) {
    return (
      <Page title="User" description={back} className="max-w-5xl">
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
      title={user.username ?? "User"}
      description={back}
      className="max-w-5xl"
      actions={
        isMe && user.staff ? (
          <span className="text-xs text-muted-foreground">You</span>
        ) : (
          <ConfirmButton
            variant={user.staff ? "outline" : "default"}
            disabled={staff.isPending}
            title={user.staff ? "Remove staff access?" : "Make staff?"}
            confirmLabel={user.staff ? "Remove staff" : "Make staff"}
            onConfirm={() => staff.mutate(!user.staff)}
            description={
              <span>
                {user.staff
                  ? "They lose the admin section and access to workspaces they aren't a member of."
                  : "Staff can open every workspace, change plans and prices, and manage other staff."}
              </span>
            }
          >
            {user.staff ? "Remove staff" : "Make staff"}
          </ConfirmButton>
        )
      }
    >
      <div className="flex items-center gap-3">
        <UserAvatar name={user.username} avatarUrl={user.avatar_url} className="size-12" />
        <div className="flex flex-col gap-0.5">
          <span className="flex items-center gap-2 font-semibold">
            {user.username ?? "No sign-in"}
            {user.staff && <Badge variant="secondary">Staff</Badge>}
            {user.disabled_at && <Badge variant="destructive">Disabled</Badge>}
          </span>
          <code className="text-xs text-muted-foreground">{user.id}</code>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-3 md:grid-cols-3">
        <Stat label="Joined" value={formatDate(user.created_at)} />
        <Stat
          label="Last active"
          value={user.last_active_at ? formatRelative(user.last_active_at) : "Never"}
        />
        <Stat label="Workspaces" value={user.workspaces} />
      </div>

      {!isMe && <AccountActions userId={userId} user={user} />}

      <Section title="Sign-ins">
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
                since {formatDate(connection.created_at)}
              </span>
            </li>
          ))}
        </ul>
      </Section>

      <Section title="Workspaces">
        {data.workspaces.length === 0 ? (
          <p className="px-5 pb-5 text-sm text-muted-foreground">Not a member of any.</p>
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
        result.sessions === 1
          ? "Signed out of 1 session"
          : `Signed out of ${result.sessions} sessions`,
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
      toast.success("They can sign in again");
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section title="Account" description="Recorded in the staff audit log.">
      <div className="flex flex-col gap-3 px-5 pb-5 text-sm">
        {user.disabled_at && (
          <p>
            <strong>Disabled</strong> {formatRelative(user.disabled_at)}: {user.disabled_reason}
          </p>
        )}
        <div className="flex flex-wrap gap-2">
          <ConfirmButton
            variant="outline"
            disabled={signOut.isPending}
            title="Sign them out everywhere?"
            confirmLabel="Sign out everywhere"
            onConfirm={() => signOut.mutate()}
            description={<span>Every browser they're signed in on. They can sign in again.</span>}
          >
            Sign out everywhere
          </ConfirmButton>
          {user.disabled_at ? (
            <ConfirmButton
              variant="outline"
              disabled={enable.isPending}
              title="Enable the account?"
              confirmLabel="Enable"
              onConfirm={() => enable.mutate()}
              description={<span>They can sign in again.</span>}
            >
              Enable account
            </ConfirmButton>
          ) : (
            !user.staff && (
              <ReasonDialog
                title="Disable this account?"
                description="They're signed out everywhere and can't sign in until enabled. Their workspaces keep running; suspend those separately."
                confirmLabel="Disable"
                onConfirm={async (reason) => {
                  const result = await unwrap(
                    api.PUT("/admin/users/{user_id}/disabled", {
                      params: { path: { user_id: userId } },
                      body: { reason },
                    }),
                  );
                  await refresh(result);
                  toast.success("Account disabled");
                }}
              >
                <Button
                  variant="outline"
                  className="border-destructive/50 text-destructive-foreground"
                >
                  Disable account…
                </Button>
              </ReasonDialog>
            )
          )}
        </div>
      </div>
    </Section>
  );
}
