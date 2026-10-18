import { useMutation, useQuery, useQueryClient, useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { Building2, Check } from "lucide-react";
import { ApiRequestError, api, unwrap } from "@/api/client";
import { meQuery, queryKeys, teamInvitePreviewQuery } from "@/api/queries";
import { MinimalHeader } from "@/components/minimal-header";
import { NoticeBanners } from "@/components/notice-banner";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { providerLabel, UserAvatar } from "@/components/user-avatar";
import { errorMessage } from "@/lib/errors";
import { ORGANIZATION_ROLE_DESCRIPTIONS, ORGANIZATION_ROLE_LABELS } from "@/lib/organizations";
import { formatRelative } from "@/lib/time";

export const Route = createFileRoute("/_app/org-invite/$token")({
  component: TeamInvitePage,
});

const GONE = {
  expired: "This invite has expired",
  revoked: "This invite was revoked",
  used: "This invite has already been used",
  open: "",
} as const;

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
        <section className="flex w-full max-w-115 flex-col gap-5.5 rounded-3xl border bg-card p-6 md:p-8">
          {preview.isPending && <Skeleton className="h-56 rounded-2xl" />}

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
                <span className="flex size-12 items-center justify-center rounded-xl bg-primary-soft text-primary-soft-foreground">
                  <Building2 className="size-6" />
                </span>
                <div className="flex flex-col gap-1">
                  <span className="text-sm text-muted-foreground">
                    {preview.data.invited_by ? (
                      <>
                        <span className="font-semibold text-foreground">
                          {preview.data.invited_by}
                        </span>{" "}
                        invited you to the team of
                      </>
                    ) : (
                      "You’re invited to the team of"
                    )}
                  </span>
                  <span className="text-2xl font-bold">{preview.data.organization_name}</span>
                </div>
              </div>

              {preview.data.already_member ? (
                <Problem
                  title="You’re already on the team"
                  detail="Open the organization to carry on."
                  action={
                    <Button asChild className="h-11 rounded-xl">
                      <Link
                        to="/orgs/$organization/clients"
                        params={{ organization: preview.data.organization_slug }}
                      >
                        Open {preview.data.organization_name}
                      </Link>
                    </Button>
                  }
                />
              ) : preview.data.status !== "open" ? (
                <Problem title={GONE[preview.data.status]} detail="Ask for a new invite link." />
              ) : (
                <>
                  <div className="flex flex-col gap-3 rounded-2xl border border-divider bg-sunken p-4">
                    <span className="flex items-center justify-between">
                      <span className="text-sm font-semibold">You’ll join as</span>
                      <span className="rounded-md bg-primary-soft px-2 py-0.5 text-xs font-bold tracking-wider text-primary-soft-foreground uppercase">
                        {ORGANIZATION_ROLE_LABELS[preview.data.role]}
                      </span>
                    </span>
                    <span className="flex items-start gap-2.5 text-sm">
                      <Check className="mt-0.5 size-4 shrink-0 text-live" />
                      {ORGANIZATION_ROLE_DESCRIPTIONS[preview.data.role]}
                    </span>
                    <span className="flex items-start gap-2.5 text-sm">
                      <Check className="mt-0.5 size-4 shrink-0 text-live" />
                      See and run every client workspace of the organization
                    </span>
                  </div>

                  <span className="flex items-center gap-2 text-sm text-muted-foreground">
                    <UserAvatar
                      name={connection?.username}
                      avatarUrl={connection?.avatar_url}
                      className="size-6"
                    />
                    Signed in as {connection?.username} · {providerLabel(connection?.provider)}
                  </span>

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
