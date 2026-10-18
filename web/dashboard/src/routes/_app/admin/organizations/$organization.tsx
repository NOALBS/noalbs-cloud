import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { ChevronLeft, Link2 } from "lucide-react";
import { useCallback, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { adminOrganizationClientsQuery, adminOrganizationQuery, queryKeys } from "@/api/queries";
import type { AdminOrganizationDetail, CreatedTeamInvite, OrganizationRole } from "@/api/types";
import { Pager, SearchBox } from "@/components/admin/list-controls";
import { OrganizationLimitsForm } from "@/components/admin/organization-limits-form";
import { ReasonDialog } from "@/components/admin/reason-dialog";
import { ConfirmButton } from "@/components/billing/confirm-button";
import { CopyField } from "@/components/copy-field";
import { Page, Section } from "@/components/page";
import { TypeToConfirm } from "@/components/type-to-confirm";
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
import { UsageBar } from "@/components/usage-bar";
import { providerLabel, UserAvatar } from "@/components/user-avatar";
import { errorMessage } from "@/lib/errors";
import {
  allocationLabel,
  belowUsage,
  clientState,
  type LimitsForm,
  limitsForm,
  ORGANIZATION_ROLE_LABELS,
  parseLimits,
  poolUsage,
  teamInviteUrl,
} from "@/lib/organizations";
import { formatDate, formatRelative } from "@/lib/time";

export const Route = createFileRoute("/_app/admin/organizations/$organization")({
  component: OrganizationPage,
});

function OrganizationPage() {
  const { organization: organizationId } = Route.useParams();
  const detail = useQuery(adminOrganizationQuery(organizationId));
  const back = (
    <Link
      to="/admin/organizations"
      className="flex items-center gap-1 text-sm font-medium text-primary"
    >
      <ChevronLeft className="size-4" />
      Organizations
    </Link>
  );
  const data = detail.data;
  if (!data) {
    return (
      <Page title="Organization" description={back} className="max-w-5xl">
        {detail.isError ? (
          <p className="text-sm">{errorMessage(detail.error)}</p>
        ) : (
          <Skeleton className="h-40" />
        )}
      </Page>
    );
  }
  const { organization } = data;
  const pool = poolUsage(organization);

  return (
    <Page
      title={organization.name}
      description={
        <span className="flex flex-wrap items-center gap-x-3 gap-y-1">
          {back}
          <span className="font-mono text-xs">/orgs/{organization.url_slug}</span>
          <span className="text-xs">Created {formatDate(organization.created_at)}</span>
          {organization.suspension && <Badge variant="destructive">Suspended</Badge>}
        </span>
      }
      className="max-w-5xl"
    >
      <Section title="Pool" description="What its clients use of the switchers it was given.">
        <div className="flex flex-col gap-2 px-5 pb-5">
          <UsageBar label="Switchers" used={pool.used} max={pool.max} />
          {pool.over > 0 && (
            <p className="text-sm text-low">
              {pool.over} over the pool. They keep running; no new switchers fit until usage drops.
            </p>
          )}
        </div>
      </Section>

      <Limits detail={data} />
      <Team detail={data} />
      <Clients detail={data} />
      <Suspension detail={data} />
      <Delete detail={data} />
    </Page>
  );
}

/** Keeps the page's data in step after a change. */
function useDetailUpdate(organizationId: string) {
  const queryClient = useQueryClient();
  return (result: AdminOrganizationDetail) => {
    queryClient.setQueryData(queryKeys.adminOrganization(organizationId), result);
    void queryClient.invalidateQueries({ queryKey: queryKeys.adminOrganizations });
    void queryClient.invalidateQueries({ queryKey: queryKeys.organizations });
  };
}

function Limits({ detail }: { detail: AdminOrganizationDetail }) {
  const { organization } = detail;
  const [form, setForm] = useState<LimitsForm>(limitsForm(organization.limits));
  const parsed = parseLimits(form);
  const update = useDetailUpdate(organization.id);
  const save = useMutation({
    mutationFn: () => {
      if (!parsed) throw new Error("Check the limits");
      return unwrap(
        api.PUT("/admin/organizations/{organization_id}", {
          params: { path: { organization_id: organization.id } },
          body: { name: organization.name, limits: parsed },
        }),
      );
    },
    onSuccess: (result) => {
      update(result);
      toast.success("Limits saved");
    },
  });
  const changed = JSON.stringify(parsed) !== JSON.stringify(organization.limits);
  const tooLow = parsed !== null && belowUsage(parsed.max_switchers, organization.usage.switchers);

  return (
    <Section
      title="Limits"
      description="Comped by staff: there is no subscription behind them yet. Every change applies to all its clients at once."
    >
      <div className="flex flex-col gap-4 px-5 pb-5">
        <OrganizationLimitsForm
          value={form}
          onChange={setForm}
          invalid={parsed === null}
          switchersInUse={organization.usage.switchers}
        >
          {save.error && <p className="text-sm text-destructive">{errorMessage(save.error)}</p>}
          <div>
            <Button
              onClick={() => save.mutate()}
              disabled={!parsed || !changed || tooLow || save.isPending}
            >
              {save.isPending ? "Saving…" : "Save limits"}
            </Button>
          </div>
        </OrganizationLimitsForm>
      </div>
    </Section>
  );
}

function Team({ detail }: { detail: AdminOrganizationDetail }) {
  const { organization } = detail;
  const queryClient = useQueryClient();
  const [role, setRole] = useState<OrganizationRole>("admin");
  const [created, setCreated] = useState<CreatedTeamInvite | null>(null);

  const invite = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/admin/organizations/{organization_id}/invites", {
          params: { path: { organization_id: organization.id } },
          body: { role, expires_in_days: 7 },
        }),
      ),
    onSuccess: async (result) => {
      setCreated(result);
      await queryClient.invalidateQueries({ queryKey: queryKeys.teamInvites(organization.id) });
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title="Team"
      description="Its own admins manage the rest. Make the first invite here and send them the link."
    >
      <ul>
        {detail.team.length === 0 && (
          <li className="border-t border-divider px-5 py-3 text-sm text-muted-foreground">
            Nobody yet.
          </li>
        )}
        {detail.team.map((member) => (
          <li
            key={member.user_id}
            className="flex items-center gap-3 border-t border-divider px-5 py-2.5"
          >
            <UserAvatar name={member.username} avatarUrl={member.avatar_url} className="size-8" />
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-sm font-semibold">
                {member.username ?? "Unknown user"}
              </span>
              <span className="text-xs text-muted-foreground">
                {[providerLabel(member.provider), `joined ${formatDate(member.joined_at)}`]
                  .filter(Boolean)
                  .join(" · ")}
              </span>
            </span>
            <Badge variant="secondary">{ORGANIZATION_ROLE_LABELS[member.role]}</Badge>
          </li>
        ))}
      </ul>
      <div className="flex flex-wrap items-center gap-2.5 border-t border-divider px-5 py-4">
        <Select value={role} onValueChange={(value) => setRole(value as OrganizationRole)}>
          <SelectTrigger className="w-36" aria-label="Role for the invite">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {(["admin", "member"] as const).map((value) => (
              <SelectItem key={value} value={value}>
                {ORGANIZATION_ROLE_LABELS[value]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button variant="outline" onClick={() => invite.mutate()} disabled={invite.isPending}>
          <Link2 />
          Create invite link
        </Button>
      </div>
      {created && (
        <div className="mx-5 mb-5 flex flex-col gap-2 rounded-xl border border-divider bg-sunken p-3.5">
          <p className="text-sm">
            New {ORGANIZATION_ROLE_LABELS[created.invite.role].toLowerCase()} invite. Copy it now:
            it won’t be shown again, works once and expires{" "}
            {formatRelative(created.invite.expires_at)}.
          </p>
          <CopyField
            value={teamInviteUrl(window.location.origin, created.token)}
            label="invite link"
            secret
          />
        </div>
      )}
    </Section>
  );
}

function Clients({ detail }: { detail: AdminOrganizationDetail }) {
  const organization = detail.organization;
  const total = organization.usage.clients;
  const [q, setQ] = useState("");
  const [page, setPage] = useState(1);
  const clients = useQuery(adminOrganizationClientsQuery(organization.id, { q, page }));
  const search = useCallback((value: string) => {
    setQ(value);
    setPage(1);
  }, []);

  return (
    <Section
      title="Clients"
      description={`${total} workspace${total === 1 ? "" : "s"}. Staff open one like any workspace: with its support access.`}
      actions={
        total > 0 && (
          <Button variant="outline" size="sm" asChild>
            <Link to="/admin/workspaces" search={{ organization: organization.id }}>
              Open in Workspaces
            </Link>
          </Button>
        )
      }
    >
      {total > 0 && (
        <div className="px-5 pb-3">
          <SearchBox value={q} onSearch={search} placeholder="Search by name, URL or ID" />
        </div>
      )}
      <ul>
        {clients.data?.items.length === 0 && (
          <li className="border-t border-divider px-5 py-3 text-sm text-muted-foreground">
            {q ? "No clients match." : "No clients yet."}
          </li>
        )}
        {clients.data?.items.map((client) => (
          <li
            key={client.id}
            className="flex flex-wrap items-center gap-3 border-t border-divider px-5 py-2.5 text-sm"
          >
            <Link
              to="/$workspace"
              params={{ workspace: client.url_slug }}
              className="flex-1 truncate font-medium hover:underline"
            >
              {client.name}
              <span className="ml-2 font-mono text-xs font-normal text-muted-foreground">
                /{client.url_slug}
              </span>
            </Link>
            {clientState(client) === "suspended" && <Badge variant="destructive">Suspended</Badge>}
            <span className="tabular font-mono text-xs text-muted-foreground">
              {client.switchers} / {allocationLabel(client.switcher_allocation)} ·{" "}
              {client.customers} {client.customers === 1 ? "customer" : "customers"}
            </span>
          </li>
        ))}
      </ul>
      {clients.data && (
        <div className="px-5 py-3">
          <Pager
            page={page}
            total={clients.data.total}
            perPage={clients.data.per_page}
            onPage={setPage}
          />
        </div>
      )}
    </Section>
  );
}

function Suspension({ detail }: { detail: AdminOrganizationDetail }) {
  const { organization } = detail;
  const update = useDetailUpdate(organization.id);
  const lift = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE("/admin/organizations/{organization_id}/suspension", {
          params: { path: { organization_id: organization.id } },
        }),
      ),
    onSuccess: (result) => {
      update(result);
      toast.success("Suspension lifted");
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title="Suspension"
      description="For abuse or non-payment: pauses the switchers of every client and silences their chat. The team and customers see the reason."
    >
      <div className="flex flex-wrap items-center justify-between gap-3 px-5 pb-5 text-sm">
        {organization.suspension ? (
          <>
            <span>
              <strong>Suspended</strong> {formatRelative(organization.suspension.since)}:{" "}
              {organization.suspension.reason}
            </span>
            <ConfirmButton
              variant="outline"
              disabled={lift.isPending}
              title="Lift the suspension?"
              confirmLabel="Lift"
              onConfirm={() => lift.mutate()}
              description={<span>Every client’s switchers resume and chat works again.</span>}
            >
              Lift suspension
            </ConfirmButton>
          </>
        ) : (
          <>
            <span className="text-muted-foreground">Not suspended.</span>
            <ReasonDialog
              title={`Suspend ${organization.name}?`}
              description="Every client’s switchers pause (no switching, no commands) and chat commands stop, until you lift it."
              confirmLabel="Suspend"
              onConfirm={async (reason) => {
                update(
                  await unwrap(
                    api.PUT("/admin/organizations/{organization_id}/suspension", {
                      params: { path: { organization_id: organization.id } },
                      body: { reason },
                    }),
                  ),
                );
                toast.success(`${organization.name} suspended`);
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

function Delete({ detail }: { detail: AdminOrganizationDetail }) {
  const { organization } = detail;
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const hasClients = organization.usage.clients > 0;

  return (
    <Section
      title="Delete organization"
      description="Only once it has no clients left: delete them from its Clients page first."
      className="border-destructive/40"
    >
      <div className="px-5 pb-5">
        <TypeToConfirm
          trigger={`Delete ${organization.name}`}
          title={`Delete ${organization.name}?`}
          description={<p>Its team loses access. This can’t be undone.</p>}
          phrase={organization.url_slug}
          confirmLabel="Delete organization"
          disabled={hasClients}
          onConfirm={async () => {
            await unwrap(
              api.DELETE("/admin/organizations/{organization_id}", {
                params: { path: { organization_id: organization.id } },
              }),
            );
            await navigate({ to: "/admin/organizations" });
            await queryClient.invalidateQueries({ queryKey: queryKeys.adminOrganizations });
            await queryClient.invalidateQueries({ queryKey: queryKeys.organizations });
            toast.success(`${organization.name} deleted`);
          }}
        />
      </div>
    </Section>
  );
}
