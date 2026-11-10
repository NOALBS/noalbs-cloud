import { plural, t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { Link2, ListPlus, Pencil, Plus } from "lucide-react";
import { useCallback, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { clientsQuery, queryKeys } from "@/api/queries";
import type { Client, ClientCustomer } from "@/api/types";
import { Pager, SearchBox } from "@/components/admin/list-controls";
import {
  BulkAllocationDialog,
  BulkInvitesDialog,
  ImportClientsDialog,
} from "@/components/orgs/bulk-dialogs";
import { EditClientDialog, NewClientDialog } from "@/components/orgs/client-dialog";
import { CustomerInviteDialog } from "@/components/orgs/customer-invite-dialog";
import { LimitsCard } from "@/components/orgs/limits-card";
import { Page } from "@/components/page";
import { Row, RowList } from "@/components/row-list";
import { SegmentedControl } from "@/components/segmented-control";
import { StatusDot } from "@/components/switcher-status";
import { TypeToConfirm } from "@/components/type-to-confirm";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { useOrganization } from "@/hooks/use-organization";
import {
  allocationLabel,
  BULK_MAX,
  bulkOutcome,
  CLIENT_STATE_DOT,
  CLIENT_STATES,
  type ClientState,
  clientState,
  clientStateLabels,
  isClaimed,
  parseClientsSearch,
} from "@/lib/organizations";
import { cn } from "@/lib/utils";

const customers = (): { value: ClientCustomer | "any"; label: string }[] => [
  { value: "any", label: t`Any customer state` },
  { value: "joined", label: t`Customer joined` },
  { value: "unclaimed", label: t`Not claimed` },
];

const sorts = () =>
  [
    { value: "newest", label: t`Newest first` },
    { value: "name", label: t`By name` },
  ] as const;

const CHECKBOX = "size-4 shrink-0 cursor-pointer accent-primary";

export const Route = createFileRoute("/_app/orgs/$organization/clients")({
  validateSearch: parseClientsSearch,
  component: ClientsPage,
});

function ClientsPage() {
  const { organization, isAdmin } = useOrganization();
  const { q = "", page = 1, customer, state, sort } = Route.useSearch();
  const navigate = useNavigate({ from: Route.fullPath });
  const clients = useQuery(clientsQuery(organization.id, { q, page, customer, state, sort }));
  const filtered = Boolean(q || customer || state);
  const suspended = organization.suspension !== null;
  // Picked clients, kept while paging and filtering.
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const ids = [...selected];
  const search = useCallback(
    (value: string) =>
      void navigate({
        search: (previous) => ({ ...previous, q: value || undefined, page: undefined }),
        replace: true,
      }),
    [navigate],
  );
  const filter = (patch: { customer?: ClientCustomer; state?: ClientState; sort?: "name" }) =>
    void navigate({ search: (previous) => ({ ...previous, ...patch, page: undefined }) });

  const items = clients.data?.items ?? [];
  const allOnPage = items.length > 0 && items.every((client) => selected.has(client.id));
  const toggle = (id: string, on: boolean) =>
    setSelected((current) => {
      const next = new Set(current);
      if (on) next.add(id);
      else next.delete(id);
      return next;
    });
  const togglePage = (on: boolean) =>
    setSelected((current) => {
      const next = new Set(current);
      for (const client of items) {
        if (on) next.add(client.id);
        else next.delete(client.id);
      }
      return next;
    });

  return (
    <Page
      title={t`Clients`}
      description={
        clients.data && !filtered
          ? plural(clients.data.total, {
              one: "# workspace you run for your customers.",
              other: "# workspaces you run for your customers.",
            })
          : t`The workspaces you run for your customers.`
      }
      actions={
        isAdmin && (
          <>
            <ImportClientsDialog organization={organization}>
              <Button variant="outline" disabled={suspended}>
                <ListPlus />
                <Trans>Import</Trans>
              </Button>
            </ImportClientsDialog>
            <NewClientDialog organization={organization}>
              <Button disabled={suspended}>
                <Plus />
                <Trans>New client</Trans>
              </Button>
            </NewClientDialog>
          </>
        )
      }
    >
      <LimitsCard organization={organization} />

      <div className="flex flex-wrap items-center gap-2 [&>div:first-child]:sm:max-w-40">
        <SearchBox value={q} onSearch={search} placeholder={t`Search by name, URL or ID`} />
        <SegmentedControl
          label={t`State`}
          value={state ?? "any"}
          onValueChange={(value) =>
            filter({ state: CLIENT_STATES.find((candidate) => candidate === value) })
          }
          options={[
            { value: "any", label: t`Any state` },
            ...CLIENT_STATES.map((value) => ({ value, label: clientStateLabels()[value] })),
          ]}
        />
        <Select
          value={customer ?? "any"}
          onValueChange={(value) =>
            filter({ customer: value === "any" ? undefined : (value as ClientCustomer) })
          }
        >
          <SelectTrigger className="w-auto" aria-label={t`Customer`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {customers().map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select
          value={sort ?? "newest"}
          onValueChange={(value) => filter({ sort: value === "name" ? "name" : undefined })}
        >
          <SelectTrigger className="w-auto" aria-label={t`Sort`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {sorts().map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {selected.size > 0 && (
        <BulkBar
          ids={ids}
          isAdmin={isAdmin}
          onClear={() => setSelected(new Set())}
          onDone={(done) =>
            setSelected((current) => {
              const next = new Set(current);
              for (const id of done) next.delete(id);
              return next;
            })
          }
        />
      )}

      {!clients.data ? (
        <Skeleton className="h-48 rounded-2xl" />
      ) : items.length === 0 ? (
        <p className="rounded-2xl border border-dashed px-4 py-8 text-center text-sm text-muted-foreground">
          {filtered
            ? t`No clients match.`
            : isAdmin
              ? t`No clients yet. Create one to set up their switcher and invite them.`
              : t`No clients yet.`}
        </p>
      ) : (
        <div className="flex flex-col overflow-hidden rounded-2xl border bg-card shadow-tile">
          <label className="flex items-center gap-3 px-5 py-3 text-xs text-muted-foreground">
            <input
              type="checkbox"
              className={CHECKBOX}
              checked={allOnPage}
              onChange={(event) => togglePage(event.target.checked)}
              aria-label={t`Select every client on this page`}
            />
            <Trans>Select this page</Trans>
          </label>
          <RowList>
            {items.map((client) => (
              <ClientRow
                key={client.id}
                client={client}
                isAdmin={isAdmin}
                organization={organization}
                selected={selected.has(client.id)}
                onSelect={(on) => toggle(client.id, on)}
              />
            ))}
          </RowList>
        </div>
      )}
      {clients.data && (
        <Pager
          page={page}
          total={clients.data.total}
          perPage={clients.data.per_page}
          onPage={(next) => void navigate({ search: (previous) => ({ ...previous, page: next }) })}
        />
      )}
    </Page>
  );
}

/** What can be done to the picked clients together. */
function BulkBar({
  ids,
  isAdmin,
  onClear,
  onDone,
}: {
  ids: string[];
  /** Allocation and delete are for admins; invite links are for the whole team. */
  isAdmin: boolean;
  onClear: () => void;
  onDone: (deleted: string[]) => void;
}) {
  const { organization } = useOrganization();
  const queryClient = useQueryClient();
  const tooMany = ids.length > BULK_MAX;

  const remove = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/organizations/{organization_id}/clients/bulk-delete", {
          params: { path: { organization_id: organization.id } },
          body: { workspace_ids: ids },
        }),
      ),
    onSuccess: async (result) => {
      const outcome = bulkOutcome(result.items);
      onDone(result.items.filter((item) => !item.error).map((item) => item.workspace_id));
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.clients(organization.id) }),
        queryClient.invalidateQueries({ queryKey: queryKeys.organizations }),
        queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
      ]);
      if (outcome.failed > 0) {
        toast.error(t`${outcome.ok} deleted, ${outcome.failed} could not be`);
      } else {
        toast.success(plural(outcome.ok, { one: "# client deleted", other: "# clients deleted" }));
      }
    },
  });

  return (
    <div
      role="toolbar"
      aria-label={t`Selected clients`}
      className="flex flex-wrap items-center gap-2 rounded-2xl border bg-card px-5 py-3 text-sm shadow-tile"
    >
      <span className="mr-auto font-semibold">
        <Trans>{ids.length} selected</Trans>
        {tooMany && (
          <span className="ml-2 font-normal text-low">
            <Trans>Pick at most {BULK_MAX} for one action.</Trans>
          </span>
        )}
      </span>
      <BulkInvitesDialog organization={organization} ids={ids}>
        <Button variant="outline" size="sm" disabled={tooMany}>
          <Link2 />
          <Trans>Invite links</Trans>
        </Button>
      </BulkInvitesDialog>
      {isAdmin && (
        <BulkAllocationDialog organization={organization} ids={ids}>
          <Button variant="outline" size="sm" disabled={tooMany}>
            <Pencil />
            <Trans>Allocation</Trans>
          </Button>
        </BulkAllocationDialog>
      )}
      {isAdmin && (
        <TypeToConfirm
          trigger={t`Delete`}
          title={plural(ids.length, { one: "Delete # client?", other: "Delete # clients?" })}
          description={
            <p>
              <Trans>
                Their workspaces go for good with their switchers, stream servers, history and the
                customers’ access. Nothing here can be restored.
              </Trans>
            </p>
          }
          phrase={t`delete ${ids.length}`}
          confirmLabel={t`Delete clients`}
          disabled={tooMany}
          onConfirm={() => remove.mutateAsync()}
        />
      )}
      <Button variant="ghost" size="sm" onClick={onClear}>
        <Trans>Clear</Trans>
      </Button>
    </div>
  );
}

function ClientRow({
  client,
  isAdmin,
  organization,
  selected,
  onSelect,
}: {
  client: Client;
  isAdmin: boolean;
  organization: ReturnType<typeof useOrganization>["organization"];
  selected: boolean;
  onSelect: (on: boolean) => void;
}) {
  const queryClient = useQueryClient();
  const state = clientState(client);
  const claimed = isClaimed(client);

  const remove = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE("/organizations/{organization_id}/clients/{workspace_id}", {
          params: { path: { organization_id: organization.id, workspace_id: client.id } },
        }),
      ),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.clients(organization.id) }),
        queryClient.invalidateQueries({ queryKey: queryKeys.organizations }),
        queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
      ]);
      toast.success(t`${client.name} was deleted`);
    },
  });

  return (
    <Row
      lead={
        <input
          type="checkbox"
          className={CHECKBOX}
          checked={selected}
          onChange={(event) => onSelect(event.target.checked)}
          aria-label={t`Select ${client.name}`}
        />
      }
      title={
        <Link to="/$workspace" params={{ workspace: client.url_slug }} className="hover:underline">
          {client.name}
        </Link>
      }
      meta={
        <>
          {claimed ? t`Customer joined` : t`Not claimed`}
          {` · ${client.switchers} / ${allocationLabel(client.switcher_allocation)} · /${client.url_slug}`}
        </>
      }
    >
      <span className="mr-2 flex items-center gap-1.5 text-xs text-muted-foreground">
        <StatusDot className={cn("size-2.5", CLIENT_STATE_DOT[state])} />
        {clientStateLabels()[state]}
      </span>

      <span className="flex flex-wrap items-center gap-1">
        <CustomerInviteDialog client={client}>
          <Button variant="outline" size="sm">
            <Link2 />
            <Trans>Invite</Trans>
          </Button>
        </CustomerInviteDialog>
        {isAdmin && (
          <>
            <EditClientDialog organization={organization} client={client}>
              <Button variant="ghost" size="icon" aria-label={t`Edit ${client.name}`}>
                <Pencil />
              </Button>
            </EditClientDialog>
            <TypeToConfirm
              trigger={t`Delete`}
              title={t`Delete ${client.name}?`}
              description={
                <p>
                  <Trans>
                    The workspace goes for good with its switchers, stream servers, history and the
                    customers’ access. Nothing here can be restored.
                  </Trans>
                </p>
              }
              phrase={client.url_slug}
              confirmLabel={t`Delete client`}
              onConfirm={() => remove.mutateAsync()}
            />
          </>
        )}
      </span>
    </Row>
  );
}
