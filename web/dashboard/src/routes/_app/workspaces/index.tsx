import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { Link2, Plus } from "lucide-react";
import { useCallback } from "react";
import { myWorkspacesQuery } from "@/api/queries";
import type { MyWorkspace } from "@/api/types";
import { Pager, SearchBox } from "@/components/admin/list-controls";
import { Page } from "@/components/page";
import { StatusDot } from "@/components/switcher-status";
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
import { WorkspaceTile } from "@/components/workspace-tile";
import { MEMBER_ROLES, parseWorkspacesSearch } from "@/lib/my-workspaces";
import {
  CLIENT_STATE_DOT,
  CLIENT_STATE_LABELS,
  CLIENT_STATES,
  type ClientState,
  clientState,
} from "@/lib/organizations";
import { ROLE_LABELS } from "@/lib/permissions";
import { cn } from "@/lib/utils";

const SORTS = [
  { value: "oldest", label: "Oldest first" },
  { value: "name", label: "By name" },
] as const;

export const Route = createFileRoute("/_app/workspaces/")({
  validateSearch: parseWorkspacesSearch,
  component: WorkspacesPage,
});

function WorkspacesPage() {
  const { q = "", page = 1, role, state, sort } = Route.useSearch();
  const navigate = useNavigate({ from: Route.fullPath });
  const workspaces = useQuery(
    myWorkspacesQuery({ q, page, role, state, sort: sort === "name" ? "name" : undefined }),
  );
  const filtered = Boolean(q || role || state);
  const search = useCallback(
    (value: string) =>
      void navigate({
        search: (previous) => ({ ...previous, q: value || undefined, page: undefined }),
        replace: true,
      }),
    [navigate],
  );
  const filter = (patch: Partial<{ role: typeof role; state: ClientState; sort: "name" }>) =>
    void navigate({ search: (previous) => ({ ...previous, ...patch, page: undefined }) });
  const items = workspaces.data?.items ?? [];

  return (
    <Page
      title="Workspaces"
      description={
        workspaces.data && !filtered
          ? `${workspaces.data.total} workspace${workspaces.data.total === 1 ? "" : "s"} you’re a member of.`
          : "The workspaces you’re a member of."
      }
      actions={
        <>
          <Button variant="outline" asChild>
            <Link to="/welcome" search={{ mode: "join" }}>
              <Link2 />
              Join
            </Link>
          </Button>
          <Button asChild>
            <Link to="/welcome" search={{ mode: "create" }}>
              <Plus />
              Create
            </Link>
          </Button>
        </>
      }
    >
      <div className="flex flex-wrap items-center gap-2">
        <SearchBox value={q} onSearch={search} placeholder="Search by name or URL" />
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
          value={role ?? "any"}
          onValueChange={(value) => filter({ role: MEMBER_ROLES.find((r) => r === value) })}
        >
          <SelectTrigger className="w-36" aria-label="Your role">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="any">Any role</SelectItem>
            {MEMBER_ROLES.map((value) => (
              <SelectItem key={value} value={value}>
                {ROLE_LABELS[value]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select
          value={sort ?? "oldest"}
          onValueChange={(value) => filter({ sort: value === "name" ? "name" : undefined })}
        >
          <SelectTrigger className="w-36" aria-label="Sort">
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

      {!workspaces.data ? (
        <Skeleton className="h-48 rounded-2xl" />
      ) : items.length === 0 ? (
        <p className="rounded-2xl border border-dashed px-4 py-8 text-center text-sm text-muted-foreground">
          {filtered ? "No workspaces match." : "You’re not in any workspace yet."}
        </p>
      ) : (
        <ul className="flex flex-col overflow-hidden rounded-2xl border bg-card">
          {items.map((item) => (
            <WorkspaceRow key={item.workspace.id} item={item} />
          ))}
        </ul>
      )}
      {workspaces.data && (
        <Pager
          page={page}
          total={workspaces.data.total}
          perPage={workspaces.data.per_page}
          onPage={(next) => void navigate({ search: (previous) => ({ ...previous, page: next }) })}
        />
      )}
    </Page>
  );
}

function WorkspaceRow({ item }: { item: MyWorkspace }) {
  const { workspace } = item;
  const status = clientState({
    streaming: item.streaming,
    obs_connected: item.obs_connected,
    suspended: Boolean(workspace.suspension),
  });

  return (
    <li className="border-b border-divider last:border-b-0">
      <Link
        to="/$workspace"
        params={{ workspace: workspace.url_slug }}
        className="flex flex-wrap items-center gap-x-4 gap-y-1 px-4 py-3 hover:bg-surface-2"
      >
        <WorkspaceTile seed={workspace.url_slug} name={workspace.name} />
        <span className="flex min-w-0 flex-1 basis-56 flex-col">
          <span className="truncate text-sm font-semibold">{workspace.name}</span>
          <span className="truncate text-xs text-muted-foreground">
            /{workspace.url_slug}
            {workspace.managed_by && ` · managed by ${workspace.managed_by.name}`}
          </span>
        </span>
        <span className="flex items-center gap-3 text-xs text-muted-foreground">
          {workspace.role && <Badge variant="secondary">{ROLE_LABELS[workspace.role]}</Badge>}
          <span className="tabular font-mono">
            {workspace.switcher_count} switcher{workspace.switcher_count === 1 ? "" : "s"}
          </span>
          <span className="flex min-w-28 items-center gap-1.5">
            <StatusDot className={cn("size-2.5", CLIENT_STATE_DOT[status])} />
            {CLIENT_STATE_LABELS[status]}
          </span>
        </span>
      </Link>
    </li>
  );
}
