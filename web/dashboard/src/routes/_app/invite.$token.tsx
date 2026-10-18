import { useMutation, useQuery, useQueryClient, useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { Check, X } from "lucide-react";
import { ApiRequestError, api, unwrap } from "@/api/client";
import { invitePreviewQuery, meQuery, queryKeys } from "@/api/queries";
import { MinimalHeader } from "@/components/minimal-header";
import { NoticeBanners } from "@/components/notice-banner";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { providerLabel, UserAvatar } from "@/components/user-avatar";
import { WorkspaceTile } from "@/components/workspace-tile";
import { errorMessage } from "@/lib/errors";
import { ROLE_LABELS } from "@/lib/permissions";
import { formatRelative } from "@/lib/time";

export const Route = createFileRoute("/_app/invite/$token")({
  component: InvitePage,
});

const ROLE_ABILITIES = {
  operator: {
    can: ["Switch scenes, start and stop the stream", "See bitrate, servers and activity"],
    cannot: ["Manage members, switchers or billing"],
  },
  admin: {
    can: [
      "Switch scenes, start and stop the stream",
      "Manage switchers, stream servers and members",
    ],
    cannot: ["Change the plan or billing"],
  },
  owner: { can: ["Everything"], cannot: [] },
} as const;

function InvitePage() {
  const { token } = Route.useParams();
  const { data: me } = useSuspenseQuery(meQuery);
  const preview = useQuery(invitePreviewQuery(token));
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const accept = useMutation({
    mutationFn: () => unwrap(api.POST("/invites/{token}/accept", { params: { path: { token } } })),
    onSuccess: async (joined) => {
      await queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
      await queryClient.invalidateQueries({ queryKey: queryKeys.me });
      await navigate({ to: "/$workspace", params: { workspace: joined.url_slug } });
    },
  });

  const connection = me.connections[0];

  return (
    <div className="flex min-h-dvh flex-col">
      <MinimalHeader me={me} />
      <NoticeBanners />
      <main className="flex flex-1 items-center justify-center px-4 py-10">
        <section className="flex w-full max-w-115 flex-col gap-5.5 rounded-3xl border bg-card p-6 md:p-8">
          {preview.isPending && <InviteSkeleton />}

          {preview.error && (
            <Problem
              title={
                preview.error instanceof ApiRequestError && preview.error.status === 404
                  ? "This invite link doesn’t exist"
                  : "Couldn’t open this invite"
              }
              detail="Check the link, or ask for a new invite."
            />
          )}

          {preview.data && (
            <>
              <div className="flex items-center gap-3.5">
                <WorkspaceTile
                  seed={preview.data.workspace_slug}
                  name={preview.data.workspace_name}
                  size="lg"
                />
                <div className="flex flex-col gap-1">
                  <span className="text-sm text-muted-foreground">
                    {preview.data.invited_by ? (
                      <>
                        <span className="font-semibold text-foreground">
                          {preview.data.invited_by}
                        </span>{" "}
                        invited you to
                      </>
                    ) : (
                      "You’re invited to"
                    )}
                  </span>
                  <span className="text-2xl font-bold">{preview.data.workspace_name}</span>
                </div>
              </div>

              {preview.data.already_member ? (
                <Problem
                  title="You’re already a member"
                  detail="Open the workspace to carry on."
                  action={
                    <Button asChild className="h-11 rounded-xl">
                      <Link to="/$workspace" params={{ workspace: preview.data.workspace_slug }}>
                        Open {preview.data.workspace_name}
                      </Link>
                    </Button>
                  }
                />
              ) : preview.data.status !== "open" ? (
                <Problem
                  title={
                    {
                      expired: "This invite has expired",
                      revoked: "This invite was revoked",
                      used: "This invite has already been used",
                      open: "",
                    }[preview.data.status]
                  }
                  detail="Ask for a new invite link."
                />
              ) : (
                <>
                  <div className="flex flex-col gap-3 rounded-2xl border border-divider bg-sunken p-4">
                    <span className="flex items-center justify-between">
                      <span className="text-sm font-semibold">You’ll join as</span>
                      <span className="rounded-md bg-primary-soft px-2 py-0.5 text-xs font-bold tracking-wider text-primary-soft-foreground uppercase">
                        {ROLE_LABELS[preview.data.role]}
                      </span>
                    </span>
                    {ROLE_ABILITIES[preview.data.role].can.map((line) => (
                      <span key={line} className="flex items-center gap-2.5 text-sm">
                        <Check className="size-4 text-live" />
                        {line}
                      </span>
                    ))}
                    {ROLE_ABILITIES[preview.data.role].cannot.map((line) => (
                      <span
                        key={line}
                        className="flex items-center gap-2.5 text-sm text-muted-foreground"
                      >
                        <X className="size-4" />
                        {line}
                      </span>
                    ))}
                  </div>

                  {preview.data.managed_by && (
                    <p className="rounded-2xl border border-divider p-4 text-sm text-muted-foreground">
                      <span className="font-semibold text-foreground">
                        {preview.data.managed_by}
                      </span>{" "}
                      manages this workspace. Their team can see and run everything in it, including
                      your stream and its stats. You can leave any time, and you can still create
                      workspaces of your own.
                    </p>
                  )}

                  <div className="flex items-center justify-between gap-3 text-sm text-muted-foreground">
                    <span className="flex items-center gap-2">
                      <UserAvatar
                        name={connection?.username}
                        avatarUrl={connection?.avatar_url}
                        className="size-6"
                      />
                      Signed in as {connection?.username} · {providerLabel(connection?.provider)}
                    </span>
                  </div>

                  {accept.error && (
                    <p className="text-sm text-destructive">{errorMessage(accept.error)}</p>
                  )}

                  <div className="grid grid-cols-2 gap-2.5">
                    <Button asChild variant="outline" className="h-12 rounded-xl">
                      <Link to="/">Decline</Link>
                    </Button>
                    <Button
                      className="h-12 rounded-xl"
                      onClick={() => accept.mutate()}
                      disabled={accept.isPending}
                    >
                      {accept.isPending ? "Joining…" : "Accept and join"}
                    </Button>
                  </div>
                  <span className="text-center font-mono text-xs text-subtle-foreground">
                    Invite expires {formatRelative(preview.data.expires_at)}
                  </span>
                </>
              )}
            </>
          )}
        </section>
      </main>
    </div>
  );
}

function Problem({
  title,
  detail,
  action,
}: {
  title: string;
  detail: string;
  action?: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-3">
      <h1 className="text-lg font-semibold">{title}</h1>
      <p className="text-sm text-muted-foreground">{detail}</p>
      {action ?? (
        <Button asChild variant="outline" className="h-11 rounded-xl">
          <Link to="/">Go to my workspaces</Link>
        </Button>
      )}
    </div>
  );
}

function InviteSkeleton() {
  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-3.5">
        <Skeleton className="size-12 rounded-xl" />
        <div className="flex flex-col gap-2">
          <Skeleton className="h-4 w-40" />
          <Skeleton className="h-6 w-52" />
        </div>
      </div>
      <Skeleton className="h-32 rounded-2xl" />
      <Skeleton className="h-12 rounded-xl" />
    </div>
  );
}
