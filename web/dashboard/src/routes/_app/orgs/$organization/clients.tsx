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
import { StatusDot } from "@/components/switcher-status";
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
import { useOrganization } from "@/hooks/use-organization";
import {
  allocationLabel,
  BULK_MAX,
  bulkOutcome,
  CLIENT_STATE_DOT,
  CLIENT_STATE_LABELS,
  CLIENT_STATES,
  type ClientState,
  clientState,
  isClaimed,
  parseClientsSearch,
} from "@/lib/organizations";
import { cn } from "@/lib/utils";

const CUSTOMERS: { value: ClientCustomer | "any"; label: string }[] = [
  { value: "any", label: "Any customer state" },
  { value: "joined", label: "Customer joined" },
  { value: "unclaimed", label: "Not claimed" },
];

const SORTS = [
  { value: "newest", label: "Newest first" },
  { value: "name", label: "By name" },
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
      title="Clients"
      description={
        clients.data && !filtered
          ? `${clients.data.total} workspace${clients.data.total === 1 ? "" : "s"} you run for your customers.`
          : "The workspaces you run for your customers."
      }
      actions={
        isAdmin && (
          <>
            <ImportClientsDialog organization={organization}>
              <Button variant="outline" disabled={suspended}>
                <ListPlus />
                Import
              </Button>
            </ImportClientsDialog>
            <NewClientDialog organization={organization}>
              <Button disabled={suspended}>
                <Plus />
                New client
              </Button>
            </NewClientDialog>
          </>
        )
      }
    >
      <LimitsCard organization={organization} />

      <div className="flex flex-wrap items-center gap-2">
        <SearchBox value={q} onSearch={search} placeholder="Search by name, URL or ID" />
        <Select
          value={state ?? "any"}
          onValueChange={(value) =>
            filter({ state: CLIENT_STATES.find((candidate) => candidate === value) })
          }
        >
          <SelectTrigger className="w-40" aria-label="State">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="any">Any state</SelectItem>
            {CLIENT_STATES.map((value) => (
              <SelectItem key={value} value={value}>
                {CLIENT_STATE_LABELS[value]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select
          value={customer ?? "any"}
          onValueChange={(value) =>
            filter({ customer: value === "any" ? undefined : (value as ClientCustomer) })
          }
        >
          <SelectTrigger className="w-44" aria-label="Customer">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {CUSTOMERS.map((option) => (
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
          <SelectTrigger className="w-40" aria-label="Sort">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SORTS.map((option) => (
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
            ? "No clients match."
            : `No clients yet.${isAdmin ? " Create one to set up their switcher and invite them." : ""}`}
        </p>
      ) : (
        <div className="flex flex-col overflow-hidden rounded-2xl border bg-card">
          <label className="flex items-center gap-3 border-b border-divider px-4 py-2 text-xs text-muted-foreground">
            <input
              type="checkbox"
              className={CHECKBOX}
              checked={allOnPage}
              onChange={(event) => togglePage(event.target.checked)}
              aria-label="Select every client on this page"
            />
            Select this page
          </label>
          <ul>
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
          </ul>
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
        toast.error(`${outcome.ok} deleted, ${outcome.failed} could not be`);
      } else {
        toast.success(`${outcome.ok} client${outcome.ok === 1 ? "" : "s"} deleted`);
      }
    },
  });

  return (
    <div
      role="toolbar"
      aria-label="Selected clients"
      className="flex flex-wrap items-center gap-2 rounded-2xl border border-primary-soft-border bg-primary-soft px-4 py-2.5 text-sm"
    >
      <span className="mr-auto font-semibold">
        {ids.length} selected
        {tooMany && (
          <span className="ml-2 font-normal text-low">Pick at most {BULK_MAX} for one action.</span>
        )}
      </span>
      <BulkInvitesDialog organization={organization} ids={ids}>
        <Button variant="outline" size="sm" disabled={tooMany}>
          <Link2 />
          Invite links
        </Button>
      </BulkInvitesDialog>
      {isAdmin && (
        <BulkAllocationDialog organization={organization} ids={ids}>
          <Button variant="outline" size="sm" disabled={tooMany}>
            <Pencil />
            Allocation
          </Button>
        </BulkAllocationDialog>
      )}
      {isAdmin && (
        <TypeToConfirm
          trigger="Delete"
          title={`Delete ${ids.length} client${ids.length === 1 ? "" : "s"}?`}
          description={
            <p>
              Their workspaces go for good with their switchers, stream servers, history and the
              customers’ access. Nothing here can be restored.
            </p>
          }
          phrase={`delete ${ids.length}`}
          confirmLabel="Delete clients"
          disabled={tooMany}
          onConfirm={() => remove.mutateAsync()}
        />
      )}
      <Button variant="ghost" size="sm" onClick={onClear}>
        Clear
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
      toast.success(`${client.name} was deleted`);
    },
  });

  return (
    <li className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-divider px-4 py-3 last:border-b-0">
      <input
        type="checkbox"
        className={CHECKBOX}
        checked={selected}
        onChange={(event) => onSelect(event.target.checked)}
        aria-label={`Select ${client.name}`}
      />
      <span className="flex min-w-0 flex-1 basis-56 items-center gap-3">
        <StatusDot className={cn("size-2.5", CLIENT_STATE_DOT[state])} />
        <span className="flex min-w-0 flex-col">
          <Link
            to="/$workspace"
            params={{ workspace: client.url_slug }}
            className="truncate text-sm font-semibold hover:underline"
          >
            {client.name}
          </Link>
          <span className="truncate text-xs text-muted-foreground">
            /{client.url_slug} · {CLIENT_STATE_LABELS[state]}
          </span>
        </span>
      </span>

      <span className="flex items-center gap-2 text-xs text-muted-foreground">
        <Badge variant={claimed ? "secondary" : "outline"}>
          {claimed ? "Customer joined" : "Not claimed"}
        </Badge>
        <span className="tabular font-mono">
          {client.switchers} / {allocationLabel(client.switcher_allocation)}
        </span>
      </span>

      <span className="flex items-center gap-1">
        <CustomerInviteDialog client={client}>
          <Button variant="outline" size="sm">
            <Link2 />
            Invite
          </Button>
        </CustomerInviteDialog>
        {isAdmin && (
          <>
            <EditClientDialog organization={organization} client={client}>
              <Button variant="ghost" size="icon" aria-label={`Edit ${client.name}`}>
                <Pencil />
              </Button>
            </EditClientDialog>
            <TypeToConfirm
              trigger="Delete"
              title={`Delete ${client.name}?`}
              description={
                <p>
                  The workspace goes for good with its switchers, stream servers, history and the
                  customers’ access. Nothing here can be restored.
                </p>
              }
              phrase={client.url_slug}
              confirmLabel="Delete client"
              onConfirm={() => remove.mutateAsync()}
            />
          </>
        )}
      </span>
    </li>
  );
}
