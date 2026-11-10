import { plural, t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { Link2, Plus } from "lucide-react";
import { useCallback } from "react";
import { myWorkspacesQuery } from "@/api/queries";
import type { MyWorkspace } from "@/api/types";
import { Pager, SearchBox } from "@/components/admin/list-controls";
import { Page } from "@/components/page";
import { Row, RowList } from "@/components/row-list";
import { SegmentedControl } from "@/components/segmented-control";
import { StatusDot } from "@/components/switcher-status";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { WorkspaceTile } from "@/components/workspace-tile";
import { MEMBER_ROLES, parseWorkspacesSearch } from "@/lib/my-workspaces";
import {
  CLIENT_STATE_DOT,
  CLIENT_STATES,
  type ClientState,
  clientState,
  clientStateLabels,
} from "@/lib/organizations";
import { roleLabels } from "@/lib/permissions";
import { cn } from "@/lib/utils";

const sorts = () =>
  [
    { value: "oldest", label: t`Oldest first` },
    { value: "name", label: t`By name` },
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
      title={t`Workspaces`}
      description={
        workspaces.data && !filtered
          ? plural(workspaces.data.total, {
              one: "# workspace you’re a member of.",
              other: "# workspaces you’re a member of.",
            })
          : t`The workspaces you’re a member of.`
      }
      actions={
        <>
          <Button variant="outline" asChild>
            <Link to="/welcome" search={{ mode: "join" }}>
              <Link2 />
              <Trans>Join</Trans>
            </Link>
          </Button>
          <Button asChild>
            <Link to="/welcome" search={{ mode: "create" }}>
              <Plus />
              <Trans>Create</Trans>
            </Link>
          </Button>
        </>
      }
    >
      <div className="flex flex-wrap items-center gap-2">
        <SearchBox value={q} onSearch={search} placeholder={t`Search by name or URL`} />
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
        <SegmentedControl
          label={t`Your role`}
          value={role ?? "any"}
          onValueChange={(value) => filter({ role: MEMBER_ROLES.find((r) => r === value) })}
          options={[
            { value: "any", label: t`Any role` },
            ...MEMBER_ROLES.map((value) => ({ value, label: roleLabels()[value] })),
          ]}
        />
        <SegmentedControl
          label={t`Sort`}
          value={sort ?? "oldest"}
          onValueChange={(value) => filter({ sort: value === "name" ? "name" : undefined })}
          options={sorts()}
        />
      </div>

      {!workspaces.data ? (
        <Skeleton className="h-48 rounded-2xl" />
      ) : items.length === 0 ? (
        <p className="rounded-2xl border bg-card px-5 py-8 text-center text-sm text-muted-foreground shadow-tile">
          {filtered ? t`No workspaces match.` : t`You’re not in any workspace yet.`}
        </p>
      ) : (
        <RowList className="overflow-hidden rounded-2xl border bg-card shadow-tile">
          {items.map((item) => (
            <WorkspaceRow key={item.workspace.id} item={item} />
          ))}
        </RowList>
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
    <Row
      className="relative first:border-t-0 hover:bg-surface-2"
      lead={<WorkspaceTile seed={workspace.url_slug} name={workspace.name} />}
      title={
        <Link
          to="/$workspace"
          params={{ workspace: workspace.url_slug }}
          className="after:absolute after:inset-0"
        >
          {workspace.name}
        </Link>
      }
      meta={
        <>
          {workspace.role && <>{roleLabels()[workspace.role]} · </>}
          {plural(workspace.switcher_count, { one: "# switcher", other: "# switchers" })}
          {t` · Plan ${workspace.plan_code}`}
          {` · /${workspace.url_slug}`}
          {workspace.managed_by && t` · managed by ${workspace.managed_by.name}`}
        </>
      }
    >
      <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <StatusDot className={cn("size-2.5", CLIENT_STATE_DOT[status])} />
        {clientStateLabels()[status]}
      </span>
    </Row>
  );
}
