import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient, useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { Building2, Check } from "lucide-react";
import { ApiRequestError, api, unwrap } from "@/api/client";
import { meQuery, queryKeys, teamInvitePreviewQuery } from "@/api/queries";
import { MinimalHeader } from "@/components/minimal-header";
import { NoticeBanners } from "@/components/notice-banner";
import { Row, RowList } from "@/components/row-list";
import { SettingsRow } from "@/components/settings-row";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { providerLabel, UserAvatar } from "@/components/user-avatar";
import { errorMessage } from "@/lib/errors";
import { organizationRoleDescriptions, organizationRoleLabels } from "@/lib/organizations";
import { formatRelative } from "@/lib/time";

export const Route = createFileRoute("/_app/org-invite/$token")({
  component: TeamInvitePage,
});

const gone = () =>
  ({
    expired: t`This invite has expired`,
    revoked: t`This invite was revoked`,
    used: t`This invite has already been used`,
    open: "",
  }) as const;

function TeamInvitePage() {
  const { token } = Route.useParams();
  const { data: me } = useSuspenseQuery(meQuery);
  const preview = useQuery(teamInvitePreviewQuery(token));
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const accept = useMutation({
    mutationFn: () =>
      unwrap(api.POST("/organization-invites/{token}/accept", { params: { path: { token } } })),
    onSuccess: async (joined) => {
      await queryClient.invalidateQueries({ queryKey: queryKeys.organizations });
      await navigate({
        to: "/orgs/$organization/clients",
        params: { organization: joined.url_slug },
      });
    },
  });

  const connection = me.connections[0];

  return (
    <div className="flex min-h-dvh flex-col">
      <MinimalHeader me={me} />
      <NoticeBanners />
      <main className="flex flex-1 items-center justify-center px-4 py-10">
        <section className="flex w-full max-w-lg flex-col overflow-hidden rounded-2xl border bg-card shadow-tile [&>*]:px-5 [&>*]:py-3 [&>*+*]:border-t [&>*+*]:border-divider">
          {preview.isPending && <Skeleton className="h-56 rounded-2xl" />}

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
                <span className="shrink-0 text-muted-foreground">
                  <Building2 className="size-6" />
                </span>
                <div className="flex min-w-0 flex-col gap-1 wrap-anywhere">
                  <span className="text-sm text-muted-foreground">
                    {preview.data.invited_by ? (
                      <>
                        <Trans>
                          <span className="font-semibold text-foreground">
                            {preview.data.invited_by}
                          </span>{" "}
                          invited you to the team of
                        </Trans>
                      </>
                    ) : (
                      t`You’re invited to the team of`
                    )}
                  </span>
                  <h1 className="font-display text-2xl font-extrabold">
                    {preview.data.organization_name}
                  </h1>
                </div>
              </div>

              {preview.data.already_member ? (
                <Problem
                  title={t`You’re already on the team`}
                  detail={t`Open the organization to carry on.`}
                  action={
                    <Button asChild className="h-11 rounded-xl">
                      <Link
                        to="/orgs/$organization/clients"
                        params={{ organization: preview.data.organization_slug }}
                      >
                        <Trans>Open {preview.data.organization_name}</Trans>
                      </Link>
                    </Button>
                  }
                />
              ) : preview.data.status !== "open" ? (
                <Problem
                  title={gone()[preview.data.status]}
                  detail={t`Ask for a new invite link.`}
                />
              ) : (
                <>
                  <div className="px-0! py-0!">
                    <SettingsRow label={t`You’ll join as`}>
                      <span className="text-sm text-muted-foreground">
                        {organizationRoleLabels()[preview.data.role]}
                      </span>
                    </SettingsRow>
                    <RowList>
                      <Row
                        lead={<Check className="size-4 text-muted-foreground" />}
                        title={
                          <span className="font-normal">
                            {organizationRoleDescriptions()[preview.data.role]}
                          </span>
                        }
                      />
                      <Row
                        lead={<Check className="size-4 text-muted-foreground" />}
                        title={
                          <span className="font-normal">
                            <Trans>See and run every client workspace of the organization</Trans>
                          </span>
                        }
                      />
                    </RowList>
                  </div>

                  <span className="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
                    <UserAvatar
                      name={connection?.username}
                      avatarUrl={connection?.avatar_url}
                      className="size-6"
                    />
                    <Trans>
                      Signed in as {connection?.username} · {providerLabel(connection?.provider)}
                    </Trans>
                  </span>

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
