import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { Building2, X } from "lucide-react";
import { useCallback } from "react";
import { adminOrganizationQuery, adminWorkspacesQuery } from "@/api/queries";
import { Pager, SearchBox } from "@/components/admin/list-controls";
import { Page } from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { formatDate } from "@/lib/time";
import { cn } from "@/lib/utils";

interface ListSearch {
  q?: string;
  page?: number;
  /** Only this organization's client workspaces. */
  organization?: string;
}

export const Route = createFileRoute("/_app/admin/workspaces/")({
  validateSearch: (search: Record<string, unknown>): ListSearch => ({
    q: typeof search.q === "string" && search.q ? search.q : undefined,
    page: Number(search.page) > 1 ? Number(search.page) : undefined,
    organization:
      typeof search.organization === "string" && search.organization
        ? search.organization
        : undefined,
  }),
  component: WorkspacesPage,
});

function WorkspacesPage() {
  const { q = "", page = 1, organization } = Route.useSearch();
  const navigate = useNavigate({ from: Route.fullPath });
  const workspaces = useQuery(adminWorkspacesQuery(q, page, organization));
  const filteredBy = useQuery({
    ...adminOrganizationQuery(organization ?? ""),
    enabled: Boolean(organization),
  });
  const search = useCallback(
    (value: string) =>
      void navigate({ search: { q: value || undefined, organization }, replace: true }),
    [navigate, organization],
  );

  return (
    <Page
      title="Workspaces"
      description={workspaces.data ? `${workspaces.data.total} workspaces` : undefined}
      className="max-w-6xl"
    >
      <div className="flex flex-wrap items-center gap-2">
        <SearchBox value={q} onSearch={search} placeholder="Search by name, slug or ID" />
        {organization && (
          <Link
            to="/admin/workspaces"
            search={{ q: q || undefined }}
            className="flex min-h-9 items-center gap-1.5 rounded-full border border-divider bg-surface-2 px-3 text-sm"
            title="Show every workspace"
          >
            <Building2 className="size-4" />
            Clients of {filteredBy.data?.organization.name ?? "…"}
            <X className="size-3.5 text-muted-foreground" />
          </Link>
        )}
      </div>
      {!workspaces.data ? (
        <Skeleton className="h-64" />
      ) : workspaces.data.items.length === 0 ? (
        <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
          No workspaces match.
        </p>
      ) : (
        <ul className="flex flex-col overflow-hidden rounded-2xl border bg-card">
          <li className="hidden grid-cols-[1fr_10rem_7rem_6rem_8rem] gap-4 border-b border-divider px-4 py-2 text-xs tracking-wider text-muted-foreground uppercase md:grid">
            <span>Workspace</span>
            <span>Plan / organization</span>
            <span>Members</span>
            <span>Switchers</span>
            <span>Created</span>
          </li>
          {workspaces.data.items.map((workspace) => (
            <li key={workspace.id} className="border-b border-divider last:border-b-0">
              <Link
                to="/admin/workspaces/$workspace"
                params={{ workspace: workspace.id }}
                className="grid grid-cols-[1fr_auto] items-center gap-x-4 gap-y-1 px-4 py-3 hover:bg-surface-2 md:grid-cols-[1fr_10rem_7rem_6rem_8rem]"
              >
                <span className="flex min-w-0 flex-col">
                  <span className="truncate text-sm font-medium">{workspace.name}</span>
                  <span className="truncate text-xs text-muted-foreground">
                    /{workspace.url_slug}
                    {workspace.owner_username && ` · ${workspace.owner_username}`}
                  </span>
                </span>
                <span className="flex min-w-0 items-center gap-1.5">
                  {workspace.suspended_at && <Badge variant="destructive">Suspended</Badge>}
                  {workspace.organization ? (
                    <Badge variant="outline" className="max-w-full gap-1 truncate">
                      <Building2 className="size-3 shrink-0" />
                      <span className="truncate">{workspace.organization.name}</span>
                    </Badge>
                  ) : (
                    <Badge variant="secondary" className="uppercase">
                      {workspace.plan_code}
                    </Badge>
                  )}
                  {workspace.subscription_status && (
                    <StatusDot status={workspace.subscription_status} />
                  )}
                </span>
                <span className="hidden text-sm md:block">{workspace.members}</span>
                <span className="hidden text-sm md:block">{workspace.switchers}</span>
                <span className="hidden text-sm text-muted-foreground md:block">
                  {formatDate(workspace.created_at)}
                </span>
              </Link>
            </li>
          ))}
        </ul>
      )}
      {workspaces.data && (
        <Pager
          page={page}
          total={workspaces.data.total}
          perPage={workspaces.data.per_page}
          onPage={(next) =>
            void navigate({ search: { q: q || undefined, organization, page: next } })
          }
        />
      )}
    </Page>
  );
}

function StatusDot({ status }: { status: string }) {
  return (
    <span
      title={`Subscription ${status.replace("_", " ")}`}
      className={cn(
        "size-2 rounded-full",
        status === "past_due" ? "bg-low" : status === "active" ? "bg-live" : "bg-idle",
      )}
    >
      <span className="sr-only">Subscription {status.replace("_", " ")}</span>
    </span>
  );
}
