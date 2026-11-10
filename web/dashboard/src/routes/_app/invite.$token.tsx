import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient, useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { Check, X } from "lucide-react";
import { ApiRequestError, api, unwrap } from "@/api/client";
import { invitePreviewQuery, meQuery, queryKeys } from "@/api/queries";
import { MinimalHeader } from "@/components/minimal-header";
import { NoticeBanners } from "@/components/notice-banner";
import { Row, RowList } from "@/components/row-list";
import { SettingsRow } from "@/components/settings-row";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { providerLabel, UserAvatar } from "@/components/user-avatar";
import { WorkspaceTile } from "@/components/workspace-tile";
import { errorMessage } from "@/lib/errors";
import { roleLabels } from "@/lib/permissions";
import { formatRelative } from "@/lib/time";

export const Route = createFileRoute("/_app/invite/$token")({
  component: InvitePage,
});

const roleAbilities = () =>
  ({
    operator: {
      can: [t`Switch scenes, start and stop the stream`, t`See bitrate, servers and activity`],
      cannot: [t`Manage members, switchers or billing`],
    },
    admin: {
      can: [
        t`Switch scenes, start and stop the stream`,
        t`Manage switchers, stream servers and members`,
      ],
      cannot: [t`Change the plan or billing`],
    },
    owner: { can: [t`Everything`], cannot: [] },
  }) as const;

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
        <section className="flex w-full max-w-lg flex-col overflow-hidden rounded-2xl border bg-card shadow-tile [&>*]:px-5 [&>*]:py-3 [&>*+*]:border-t [&>*+*]:border-divider">
          {preview.isPending && <InviteSkeleton />}

          {preview.error && (
            <Problem
              title={
                preview.error instanceof ApiRequestError && preview.error.status === 404
                  ? t`This invite link doesn’t exist`
                  : t`Couldn’t open this invite`
              }
              detail={t`Check the link, or ask for a new invite.`}
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
                <div className="flex min-w-0 flex-col gap-1 wrap-anywhere">
                  <span className="text-sm text-muted-foreground">
                    {preview.data.invited_by ? (
                      <>
                        <Trans>
                          <span className="font-semibold text-foreground">
                            {preview.data.invited_by}
                          </span>{" "}
                          invited you to
                        </Trans>
                      </>
                    ) : (
                      t`You’re invited to`
                    )}
                  </span>
                  <h1 className="font-display text-2xl font-extrabold">
                    {preview.data.workspace_name}
                  </h1>
                </div>
              </div>

              {preview.data.already_member ? (
                <Problem
                  title={t`You’re already a member`}
                  detail={t`Open the workspace to carry on.`}
                  action={
                    <Button asChild className="h-11 rounded-xl">
                      <Link to="/$workspace" params={{ workspace: preview.data.workspace_slug }}>
                        <Trans>Open {preview.data.workspace_name}</Trans>
                      </Link>
                    </Button>
                  }
                />
              ) : preview.data.status !== "open" ? (
                <Problem
                  title={
                    {
                      expired: t`This invite has expired`,
                      revoked: t`This invite was revoked`,
                      used: t`This invite has already been used`,
                      open: "",
                    }[preview.data.status]
                  }
                  detail={t`Ask for a new invite link.`}
                />
              ) : (
                <>
                  <div className="px-0! py-0!">
                    <SettingsRow label={t`You’ll join as`}>
                      <span className="text-sm text-muted-foreground">
                        {roleLabels()[preview.data.role]}
                      </span>
                    </SettingsRow>
                    <RowList>
                      {roleAbilities()[preview.data.role].can.map((line) => (
                        <Row
                          key={line}
                          lead={<Check className="size-4 text-muted-foreground" />}
                          title={<span className="font-normal">{line}</span>}
                        />
                      ))}
                      {roleAbilities()[preview.data.role].cannot.map((line) => (
                        <Row
                          key={line}
                          lead={<X className="size-4 text-muted-foreground" />}
                          title={<span className="font-normal text-muted-foreground">{line}</span>}
                        />
                      ))}
                    </RowList>
                  </div>

                  {preview.data.managed_by && (
                    <p className="text-sm text-muted-foreground">
                      <Trans>
                        <span className="font-semibold text-foreground">
                          {preview.data.managed_by}
                        </span>{" "}
                        manages this workspace. Their team can see and run everything in it,
                        including your stream and its stats. You can leave any time, and you can
                        still create workspaces of your own.
                      </Trans>
                    </p>
                  )}

                  <div className="flex items-center justify-between gap-3 text-sm text-muted-foreground">
                    <span className="flex min-w-0 flex-wrap items-center gap-2">
                      <UserAvatar
                        name={connection?.username}
                        avatarUrl={connection?.avatar_url}
                        className="size-6"
                      />
                      <Trans>
                        Signed in as {connection?.username} · {providerLabel(connection?.provider)}
                      </Trans>
                    </span>
                  </div>

                  {accept.error && (
                    <p className="text-sm text-destructive">{errorMessage(accept.error)}</p>
                  )}

                  <div className="grid gap-2.5 sm:grid-cols-2">
                    <Button asChild variant="outline" className="h-12 rounded-xl">
                      <Link to="/">
                        <Trans>Decline</Trans>
                      </Link>
                    </Button>
                    <Button
                      className="h-12 rounded-xl"
                      onClick={() => accept.mutate()}
                      disabled={accept.isPending}
                    >
                      {accept.isPending ? t`Joining…` : t`Accept and join`}
                    </Button>
                  </div>
                  <span className="text-center font-mono text-xs text-subtle-foreground">
                    <Trans>Invite expires {formatRelative(preview.data.expires_at)}</Trans>
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
      <h1 className="font-display text-lg font-bold">{title}</h1>
      <p className="text-sm text-muted-foreground">{detail}</p>
      {action ?? (
        <Button asChild variant="outline" className="h-11 rounded-xl">
          <Link to="/">
            <Trans>Go to my workspaces</Trans>
          </Link>
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
