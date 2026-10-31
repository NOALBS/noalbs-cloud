import { plural, t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { Building2, ChevronLeft, ExternalLink } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { adminProductsQuery, adminWorkspaceQuery, queryKeys } from "@/api/queries";
import type { AdminWorkspaceDetail, SupportGrant } from "@/api/types";
import { Stat } from "@/components/admin/list-controls";
import { ReasonDialog } from "@/components/admin/reason-dialog";
import { ConfirmButton } from "@/components/billing/confirm-button";
import { Page, Section } from "@/components/page";
import { BreakGlassDialog } from "@/components/support/break-glass-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { formatPrice } from "@/lib/billing";
import { errorMessage } from "@/lib/errors";
import { currentLocale } from "@/lib/locale";
import { supportLevelLabels } from "@/lib/support";
import { formatDate, formatRelative } from "@/lib/time";

export const Route = createFileRoute("/_app/admin/workspaces/$workspace")({
  component: WorkspacePage,
});

function WorkspacePage() {
  const { workspace: workspaceId } = Route.useParams();
  const detail = useQuery(adminWorkspaceQuery(workspaceId));
  const back = (
    <Link
      to="/admin/workspaces"
      className="flex items-center gap-1 text-sm font-medium text-primary"
    >
      <ChevronLeft className="size-4" />
      <Trans>Workspaces</Trans>
    </Link>
  );
  const data = detail.data;
  if (!data) {
    return (
      <Page title={t`Workspace`} description={back} className="max-w-5xl">
        {detail.isError ? (
          <p className="text-sm">{errorMessage(detail.error)}</p>
        ) : (
          <Skeleton className="h-40" />
        )}
      </Page>
    );
  }
  const workspace = data.workspace;

  return (
    <Page
      title={workspace.name}
      description={
        <span className="flex flex-wrap items-center gap-x-3 gap-y-1">
          {back}
          {workspace.organization && (
            <Link
              to="/admin/organizations/$organization"
              params={{ organization: workspace.organization.id }}
              className="flex items-center gap-1 text-sm font-medium text-primary hover:underline"
            >
              <Building2 className="size-4" />
              <Trans>Client of {workspace.organization.name}</Trans>
            </Link>
          )}
        </span>
      }
      className="max-w-5xl"
      actions={
        <>
          {workspace.stripe_customer_url && (
            <Button variant="outline" asChild>
              <a href={workspace.stripe_customer_url} target="_blank" rel="noreferrer">
                <Trans>Stripe customer</Trans>
                <ExternalLink />
              </a>
            </Button>
          )}
          <Button asChild>
            <Link to="/$workspace" params={{ workspace: workspace.url_slug }}>
              <Trans>Open workspace</Trans>
            </Link>
          </Button>
        </>
      }
    >
      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <Stat
          label={t`Plan`}
          value={workspace.organization ? "HOSTED" : workspace.plan_code.toUpperCase()}
        />
        <Stat label={t`Members`} value={workspace.members} />
        <Stat label={t`Switchers`} value={workspace.switchers} />
        <Stat label={t`Created`} value={formatDate(workspace.created_at)} />
      </div>

      <Suspension detail={data} />
      <SupportStatus detail={data} />
      {/* A client workspace has no plan or billing of its own: its organization's limits apply. */}
      {!workspace.organization && <SetPlan detail={data} />}
      {!workspace.organization && <Billing detail={data} />}

      <Section title={t`Members`}>
        <ul className="flex flex-col pb-2">
          {data.members.map((member) => (
            <li key={member.user_id}>
              <Link
                to="/admin/users/$user"
                params={{ user: member.user_id }}
                className="flex items-center gap-3 px-5 py-2.5 hover:bg-surface-2"
              >
                <span className="min-w-0 flex-1 truncate text-sm font-medium">
                  {member.username ?? t`No sign-in`}
                </span>
                <Badge variant="outline">{member.role}</Badge>
              </Link>
            </li>
          ))}
        </ul>
      </Section>

      <Section title={t`Switchers`}>
        {data.switchers.length === 0 ? (
          <p className="px-5 pb-5 text-sm text-muted-foreground">
            <Trans>None yet.</Trans>
          </p>
        ) : (
          <ul className="flex flex-col pb-2">
            {data.switchers.map((switcher) => (
              <li key={switcher.id} className="flex items-center gap-3 px-5 py-2.5">
                <span className="min-w-0 flex-1 truncate text-sm font-medium">{switcher.name}</span>
                {switcher.paused && (
                  <Badge className="bg-low/15 text-low">
                    <Trans>Paused</Trans>
                  </Badge>
                )}
                <span className="text-xs text-muted-foreground">
                  <Trans>
                    {switcher.stream_server_count} servers ·{" "}
                    {switcher.last_seen_at
                      ? t`seen ${formatRelative(switcher.last_seen_at)}`
                      : t`never connected`}
                  </Trans>
                </span>
                <ReconnectButton switcherId={switcher.id} />
              </li>
            ))}
          </ul>
        )}
      </Section>

      <ChatChannels detail={data} />
    </Page>
  );
}

/** Suspend the workspace (switchers pause, chat goes quiet) or lift it. */
function Suspension({ detail }: { detail: AdminWorkspaceDetail }) {
  const queryClient = useQueryClient();
  const { workspace } = detail;
  const done = (result: AdminWorkspaceDetail) => {
    queryClient.setQueryData(queryKeys.adminWorkspace(workspace.id), result);
    void queryClient.invalidateQueries({ queryKey: queryKeys.admin });
  };
  const lift = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE("/admin/workspaces/{workspace_id}/suspension", {
          params: { path: { workspace_id: workspace.id } },
        }),
      ),
    onSuccess: (result) => {
      done(result);
      toast.success(t`Suspension lifted`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title={t`Suspension`}
      description={t`For abuse: pauses every switcher and silences its chat links. Its members see the reason. Recorded in the audit log and the workspace's support log.`}
    >
      <div className="flex flex-wrap items-center justify-between gap-3 px-5 pb-5 text-sm">
        {workspace.suspended_at ? (
          <>
            <span>
              <strong>
                <Trans>Suspended</Trans>
              </strong>{" "}
              {formatRelative(workspace.suspended_at)}: {workspace.suspended_reason}
            </span>
            <ConfirmButton
              variant="outline"
              disabled={lift.isPending}
              title={t`Lift the suspension?`}
              confirmLabel={t`Lift`}
              onConfirm={() => lift.mutate()}
              description={
                <span>
                  <Trans>Its switchers resume and its chat links work again.</Trans>
                </span>
              }
            >
              <Trans>Lift suspension</Trans>
            </ConfirmButton>
          </>
        ) : (
          <>
            <span className="text-muted-foreground">
              <Trans>Not suspended.</Trans>
            </span>
            <ReasonDialog
              title={t`Suspend ${workspace.name}?`}
              description={t`Every switcher pauses (no switching, no commands) and chat commands stop, until you lift it. Its members see this reason.`}
              confirmLabel={t`Suspend`}
              onConfirm={async (reason) => {
                done(
                  await unwrap(
                    api.PUT("/admin/workspaces/{workspace_id}/suspension", {
                      params: { path: { workspace_id: workspace.id } },
                      body: { reason },
                    }),
                  ),
                );
                toast.success(t`${workspace.name} suspended`);
              }}
            >
              <Button
                variant="outline"
                className="border-destructive/50 text-destructive-foreground"
              >
                <Trans>Suspend…</Trans>
              </Button>
            </ReasonDialog>
          </>
        )}
      </div>
    </Section>
  );
}

/** Close a switcher's browser-source connection so it reconnects. */
function ReconnectButton({ switcherId }: { switcherId: string }) {
  const reconnect = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/admin/switchers/{switcher_id}/reconnect", {
          params: { path: { switcher_id: switcherId } },
        }),
      ),
    onSuccess: () => toast.success(t`Asked the browser source to reconnect`),
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <ConfirmButton
      className="h-8 px-3"
      variant="ghost"
      disabled={reconnect.isPending}
      title={t`Reconnect the browser source?`}
      confirmLabel={t`Reconnect`}
      onConfirm={() => reconnect.mutate()}
      description={
        <span>
          <Trans>
            Its connection closes and the browser source reconnects within a second or two. Shown in
            the workspace's support log.
          </Trans>
        </span>
      }
    >
      <Trans>Reconnect</Trans>
    </ConfirmButton>
  );
}

/** Chat channels linked to the workspace, each unlinkable everywhere. */
function ChatChannels({ detail }: { detail: AdminWorkspaceDetail }) {
  const queryClient = useQueryClient();
  const unlink = useMutation({
    mutationFn: (channelId: string) =>
      unwrap(
        api.DELETE("/admin/chat-channels/{channel_id}/links", {
          params: { path: { channel_id: channelId } },
        }),
      ),
    onSuccess: async (result) => {
      await queryClient.invalidateQueries({
        queryKey: queryKeys.adminWorkspace(detail.workspace.id),
      });
      toast.success(
        plural(result.links, {
          one: "Unlinked from # switcher",
          other: "Unlinked from # switchers",
        }),
      );
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title={t`Chat channels`}
      description={t`Unlinking a channel removes it from every switcher, in every workspace: the bot leaves it. Its owner can link it again.`}
    >
      {detail.chat_channels.length === 0 ? (
        <p className="px-5 pb-5 text-sm text-muted-foreground">
          <Trans>None linked.</Trans>
        </p>
      ) : (
        <ul className="flex flex-col pb-2">
          {detail.chat_channels.map((channel) => (
            <li key={channel.id} className="flex flex-wrap items-center gap-3 px-5 py-2.5">
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-sm font-medium">
                  {channel.display_name}{" "}
                  <span className="font-normal text-muted-foreground">({channel.platform})</span>
                </span>
                <span className="truncate text-xs text-muted-foreground">
                  {channel.links
                    .map((link) => `${link.switcher_name} · ${link.workspace_name}`)
                    .join(", ")}
                </span>
              </span>
              {channel.status === "revoked" && (
                <Badge variant="outline">
                  <Trans>Revoked</Trans>
                </Badge>
              )}
              <ConfirmButton
                className="h-8 px-3"
                variant="outline"
                disabled={unlink.isPending}
                title={t`Unlink ${channel.display_name} everywhere?`}
                confirmLabel={t`Unlink everywhere`}
                onConfirm={() => unlink.mutate(channel.id)}
                description={
                  <span>
                    <Trans>
                      From {channel.links.length} switcher(s). Each affected workspace sees it in
                      its support log.
                    </Trans>
                  </span>
                }
              >
                <Trans>Unlink everywhere</Trans>
              </ConfirmButton>
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}

/** Whether NOALBS support may open the workspace, and emergency access if not. */
/** What a grant allows and until when, in a line. */
function grantLine(grant: SupportGrant): string {
  const level = supportLevelLabels()[grant.level];
  const until = new Date(grant.expires_at).toLocaleString(currentLocale());
  const access = grant.break_glass
    ? t`Emergency access (${level}) until ${until}`
    : t`${level} until ${until}`;
  return grant.granted_by ? t`${access}, allowed by ${grant.granted_by}` : access;
}

function SupportStatus({ detail }: { detail: AdminWorkspaceDetail }) {
  const grant = detail.support_access;
  const { workspace } = detail;

  return (
    <Section
      title={t`Support access`}
      description={t`Opening the workspace needs its permission. Everything you do there is shown to its owners and admins.`}
    >
      <div className="flex flex-wrap items-center justify-between gap-3 px-5 pb-5 text-sm">
        <span>
          {grant
            ? grantLine(grant)
            : t`No permission: ask an owner or admin to allow support access.`}
        </span>
        {!grant && (
          <BreakGlassDialog workspaceId={workspace.id} workspaceName={workspace.name}>
            <Button variant="outline" className="border-destructive/50 text-destructive-foreground">
              <Trans>Emergency access…</Trans>
            </Button>
          </BreakGlassDialog>
        )}
      </div>
    </Section>
  );
}

/** Comp a workspace: set its plan without billing. */
function SetPlan({ detail }: { detail: AdminWorkspaceDetail }) {
  const products = useQuery(adminProductsQuery);
  const queryClient = useQueryClient();
  const workspace = detail.workspace;
  const [code, setCode] = useState(workspace.plan_code);
  useEffect(() => setCode(workspace.plan_code), [workspace.plan_code]);
  const setPlan = useMutation({
    mutationFn: () =>
      unwrap(
        api.PUT("/admin/workspaces/{workspace_id}/plan", {
          params: { path: { workspace_id: workspace.id } },
          body: { code },
        }),
      ),
    onSuccess: async (result) => {
      queryClient.setQueryData(queryKeys.adminWorkspace(workspace.id), result);
      await queryClient.invalidateQueries({ queryKey: queryKeys.admin });
      await queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
      toast.success(t`Moved to ${code}`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title={t`Set plan`}
      description={
        detail.subscription
          ? t`This workspace pays through Stripe: its next Stripe event sets the plan back to what it pays for.`
          : t`Sets the plan directly, without billing (a comp).`
      }
    >
      <div className="flex items-center gap-2 px-5 pb-5">
        <Select value={code} onValueChange={setCode}>
          <SelectTrigger className="w-44" aria-label={t`Plan`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {products.data?.plans.map((plan) => (
              <SelectItem key={plan.code} value={plan.code}>
                {plan.name}
                {!plan.is_active && t` (hidden)`}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          onClick={() => setPlan.mutate()}
          disabled={code === workspace.plan_code || setPlan.isPending}
        >
          <Trans>Apply</Trans>
        </Button>
      </div>
    </Section>
  );
}

function Billing({ detail }: { detail: AdminWorkspaceDetail }) {
  const subscription = detail.subscription;
  if (!subscription) {
    return (
      <Section title={t`Billing`} description={t`No subscription.`}>
        {detail.addons.length > 0 && <AddonList detail={detail} />}
      </Section>
    );
  }
  return (
    <Section
      title={t`Billing`}
      description={[
        subscription.status.replace("_", " "),
        subscription.interval === "year" ? t`billed yearly` : t`billed monthly`,
        subscription.current_period_end &&
          (subscription.cancel_at_period_end
            ? t`ends ${formatDate(subscription.current_period_end)}`
            : t`renews ${formatDate(subscription.current_period_end)}`),
      ]
        .filter(Boolean)
        .join(" · ")}
      actions={
        subscription.stripe_url && (
          <a
            href={subscription.stripe_url}
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-1 text-sm font-medium text-primary hover:underline"
          >
            <Trans>Subscription in Stripe</Trans>
            <ExternalLink className="size-3.5" />
          </a>
        )
      }
    >
      <ul className="flex flex-col pb-2">
        {subscription.items.map((item) => (
          <li key={`${item.kind}-${item.code}`} className="flex items-center gap-3 px-5 py-2.5">
            <Badge variant="outline">{item.kind}</Badge>
            <span className="min-w-0 flex-1 truncate text-sm font-medium">
              {item.code}
              {item.quantity > 1 && ` × ${item.quantity}`}
            </span>
            {item.grandfathered && (
              <Badge variant="secondary">
                <Trans>Old price</Trans>
              </Badge>
            )}
            <span className="font-mono text-sm tabular">
              {item.price
                ? formatPrice(item.price.amount * item.quantity, item.price.currency)
                : "—"}
            </span>
          </li>
        ))}
        {subscription.held_plan_code && (
          <li className="px-5 py-2.5 text-sm text-muted-foreground">
            <Trans>
              Keeps {subscription.held_plan_code} until the period ends, then moves to Free.
            </Trans>
          </li>
        )}
      </ul>
      <AddonList detail={detail} />
    </Section>
  );
}

function AddonList({ detail }: { detail: AdminWorkspaceDetail }) {
  const held = detail.addons.filter((addon) => addon.held_until);
  if (held.length === 0) return null;
  return (
    <ul className="flex flex-col border-t border-divider pb-2">
      {held.map((addon) => (
        <li key={addon.code} className="px-5 py-2.5 text-sm text-muted-foreground">
          <Trans>
            {addon.code}: {addon.quantity} until {formatDate(addon.held_until ?? "")}, then{" "}
            {addon.billed}
          </Trans>
        </li>
      ))}
    </ul>
  );
}
