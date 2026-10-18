import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { Building2, ChevronLeft, ExternalLink } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { adminProductsQuery, adminWorkspaceQuery, queryKeys } from "@/api/queries";
import type { AdminWorkspaceDetail } from "@/api/types";
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
import { SUPPORT_LEVEL_LABELS } from "@/lib/support";
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
      Workspaces
    </Link>
  );
  const data = detail.data;
  if (!data) {
    return (
      <Page title="Workspace" description={back} className="max-w-5xl">
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
              Client of {workspace.organization.name}
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
                Stripe customer
                <ExternalLink />
              </a>
            </Button>
          )}
          <Button asChild>
            <Link to="/$workspace" params={{ workspace: workspace.url_slug }}>
              Open workspace
            </Link>
          </Button>
        </>
      }
    >
      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <Stat
          label="Plan"
          value={workspace.organization ? "HOSTED" : workspace.plan_code.toUpperCase()}
        />
        <Stat label="Members" value={workspace.members} />
        <Stat label="Switchers" value={workspace.switchers} />
        <Stat label="Created" value={formatDate(workspace.created_at)} />
      </div>

      <Suspension detail={data} />
      <SupportStatus detail={data} />
      {/* A client workspace has no plan or billing of its own: its organization's limits apply. */}
      {!workspace.organization && <SetPlan detail={data} />}
      {!workspace.organization && <Billing detail={data} />}

      <Section title="Members">
        <ul className="flex flex-col pb-2">
          {data.members.map((member) => (
            <li key={member.user_id}>
              <Link
                to="/admin/users/$user"
                params={{ user: member.user_id }}
                className="flex items-center gap-3 px-5 py-2.5 hover:bg-surface-2"
              >
                <span className="min-w-0 flex-1 truncate text-sm font-medium">
                  {member.username ?? "No sign-in"}
                </span>
                <Badge variant="outline">{member.role}</Badge>
              </Link>
            </li>
          ))}
        </ul>
      </Section>

      <Section title="Switchers">
        {data.switchers.length === 0 ? (
          <p className="px-5 pb-5 text-sm text-muted-foreground">None yet.</p>
        ) : (
          <ul className="flex flex-col pb-2">
            {data.switchers.map((switcher) => (
              <li key={switcher.id} className="flex items-center gap-3 px-5 py-2.5">
                <span className="min-w-0 flex-1 truncate text-sm font-medium">{switcher.name}</span>
                {switcher.paused && <Badge className="bg-low/15 text-low">Paused</Badge>}
                <span className="text-xs text-muted-foreground">
                  {switcher.stream_server_count} servers ·{" "}
                  {switcher.last_seen_at
                    ? `seen ${formatRelative(switcher.last_seen_at)}`
                    : "never connected"}
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
      toast.success("Suspension lifted");
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title="Suspension"
      description="For abuse: pauses every switcher and silences its chat links. Its members see the reason. Recorded in the audit log and the workspace's support log."
    >
      <div className="flex flex-wrap items-center justify-between gap-3 px-5 pb-5 text-sm">
        {workspace.suspended_at ? (
          <>
            <span>
              <strong>Suspended</strong> {formatRelative(workspace.suspended_at)}:{" "}
              {workspace.suspended_reason}
            </span>
            <ConfirmButton
              variant="outline"
              disabled={lift.isPending}
              title="Lift the suspension?"
              confirmLabel="Lift"
              onConfirm={() => lift.mutate()}
              description={<span>Its switchers resume and its chat links work again.</span>}
            >
              Lift suspension
            </ConfirmButton>
          </>
        ) : (
          <>
            <span className="text-muted-foreground">Not suspended.</span>
            <ReasonDialog
              title={`Suspend ${workspace.name}?`}
              description="Every switcher pauses (no switching, no commands) and chat commands stop, until you lift it. Its members see this reason."
              confirmLabel="Suspend"
              onConfirm={async (reason) => {
                done(
                  await unwrap(
                    api.PUT("/admin/workspaces/{workspace_id}/suspension", {
                      params: { path: { workspace_id: workspace.id } },
                      body: { reason },
                    }),
                  ),
                );
                toast.success(`${workspace.name} suspended`);
              }}
            >
              <Button
                variant="outline"
                className="border-destructive/50 text-destructive-foreground"
              >
                Suspend…
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
    onSuccess: () => toast.success("Asked the browser source to reconnect"),
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <ConfirmButton
      className="h-8 px-3"
      variant="ghost"
      disabled={reconnect.isPending}
      title="Reconnect the browser source?"
      confirmLabel="Reconnect"
      onConfirm={() => reconnect.mutate()}
      description={
        <span>
          Its connection closes and the browser source reconnects within a second or two. Shown in
          the workspace's support log.
        </span>
      }
    >
      Reconnect
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
        result.links === 1 ? "Unlinked from 1 switcher" : `Unlinked from ${result.links} switchers`,
      );
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title="Chat channels"
      description="Unlinking a channel removes it from every switcher, in every workspace: the bot leaves it. Its owner can link it again."
    >
      {detail.chat_channels.length === 0 ? (
        <p className="px-5 pb-5 text-sm text-muted-foreground">None linked.</p>
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
              {channel.status === "revoked" && <Badge variant="outline">Revoked</Badge>}
              <ConfirmButton
                className="h-8 px-3"
                variant="outline"
                disabled={unlink.isPending}
                title={`Unlink ${channel.display_name} everywhere?`}
                confirmLabel="Unlink everywhere"
                onConfirm={() => unlink.mutate(channel.id)}
                description={
                  <span>
                    From {channel.links.length} switcher(s). Each affected workspace sees it in its
                    support log.
                  </span>
                }
              >
                Unlink everywhere
              </ConfirmButton>
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}

/** Whether NOALBS support may open the workspace, and emergency access if not. */
function SupportStatus({ detail }: { detail: AdminWorkspaceDetail }) {
  const grant = detail.support_access;
  const { workspace } = detail;

  return (
    <Section
      title="Support access"
      description="Opening the workspace needs its permission. Everything you do there is shown to its owners and admins."
    >
      <div className="flex flex-wrap items-center justify-between gap-3 px-5 pb-5 text-sm">
        <span>
          {grant
            ? `${grant.break_glass ? `Emergency access, ${SUPPORT_LEVEL_LABELS[grant.level].toLowerCase()}` : SUPPORT_LEVEL_LABELS[grant.level]} until ${new Date(grant.expires_at).toLocaleString(currentLocale())}${grant.granted_by ? `, allowed by ${grant.granted_by}` : ""}`
            : "No permission: ask an owner or admin to allow support access."}
        </span>
        {!grant && (
          <BreakGlassDialog workspaceId={workspace.id} workspaceName={workspace.name}>
            <Button variant="outline" className="border-destructive/50 text-destructive-foreground">
              Emergency access…
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
      toast.success(`Moved to ${code}`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title="Set plan"
      description={
        detail.subscription
          ? "This workspace pays through Stripe: its next Stripe event sets the plan back to what it pays for."
          : "Sets the plan directly, without billing (a comp)."
      }
    >
      <div className="flex items-center gap-2 px-5 pb-5">
        <Select value={code} onValueChange={setCode}>
          <SelectTrigger className="w-44" aria-label="Plan">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {products.data?.plans.map((plan) => (
              <SelectItem key={plan.code} value={plan.code}>
                {plan.name}
                {!plan.is_active && " (hidden)"}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          onClick={() => setPlan.mutate()}
          disabled={code === workspace.plan_code || setPlan.isPending}
        >
          Apply
        </Button>
      </div>
    </Section>
  );
}

function Billing({ detail }: { detail: AdminWorkspaceDetail }) {
  const subscription = detail.subscription;
  if (!subscription) {
    return (
      <Section title="Billing" description="No subscription.">
        {detail.addons.length > 0 && <AddonList detail={detail} />}
      </Section>
    );
  }
  return (
    <Section
      title="Billing"
      description={`${subscription.status.replace("_", " ")} · billed ${
        subscription.interval === "year" ? "yearly" : "monthly"
      }${
        subscription.current_period_end
          ? subscription.cancel_at_period_end
            ? ` · ends ${formatDate(subscription.current_period_end)}`
            : ` · renews ${formatDate(subscription.current_period_end)}`
          : ""
      }`}
      actions={
        subscription.stripe_url && (
          <a
            href={subscription.stripe_url}
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-1 text-sm font-medium text-primary hover:underline"
          >
            Subscription in Stripe
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
            {item.grandfathered && <Badge variant="secondary">Old price</Badge>}
            <span className="font-mono text-sm tabular">
              {item.price
                ? formatPrice(item.price.amount * item.quantity, item.price.currency)
                : "—"}
            </span>
          </li>
        ))}
        {subscription.held_plan_code && (
          <li className="px-5 py-2.5 text-sm text-muted-foreground">
            Keeps {subscription.held_plan_code} until the period ends, then moves to Free.
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
          {addon.code}: {addon.quantity} until {formatDate(addon.held_until ?? "")}, then{" "}
          {addon.billed}
        </li>
      ))}
    </ul>
  );
}
