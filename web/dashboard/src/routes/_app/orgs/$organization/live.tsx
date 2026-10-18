import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { useCallback } from "react";
import { organizationLiveQuery } from "@/api/queries";
import { Pager, SearchBox, Stat } from "@/components/admin/list-controls";
import { Page } from "@/components/page";
import { StatusDot } from "@/components/switcher-status";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useOrganization } from "@/hooks/use-organization";
import { formatUptime } from "@/lib/live-format";
import { type LiveSearch, liveTag, parseLiveSearch } from "@/lib/organizations";
import { cn } from "@/lib/utils";

const SHOW: { value: LiveSearch["show"]; label: string }[] = [
  { value: undefined, label: "All connected" },
  { value: "live", label: "Live" },
  { value: "obs", label: "OBS connected" },
];

export const Route = createFileRoute("/_app/orgs/$organization/live")({
  validateSearch: parseLiveSearch,
  component: LivePage,
});

function LivePage() {
  const { organization } = useOrganization();
  const { q = "", page = 1, show } = Route.useSearch();
  const navigate = useNavigate({ from: Route.fullPath });
  const live = useQuery(organizationLiveQuery(organization.id, { q, page, filter: show }));
  const search = useCallback(
    (value: string) =>
      void navigate({
        search: (previous) => ({ ...previous, q: value || undefined, page: undefined }),
        replace: true,
      }),
    [navigate],
  );
  const counts = live.data?.counts;
  const now = Date.now();

  return (
    <Page
      title="Live"
      description="Every switcher of your clients that is connected now, live ones first."
    >
      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <Stat label="Live now" value={counts?.live ?? "–"} hint={liveHint(counts)} />
        <Stat label="OBS connected" value={counts?.obs_connected ?? "–"} />
        <Stat label="Connected" value={counts?.connected ?? "–"} hint="browser source up" />
        <Stat
          label="Switchers"
          value={counts?.switchers ?? "–"}
          hint={
            counts ? `in ${counts.clients} client${counts.clients === 1 ? "" : "s"}` : undefined
          }
        />
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <SearchBox value={q} onSearch={search} placeholder="Search by client or switcher" />
        <fieldset className="flex flex-wrap gap-1" aria-label="Show">
          {SHOW.map((option) => (
            <Button
              key={option.label}
              size="sm"
              variant={show === option.value ? "secondary" : "ghost"}
              aria-pressed={show === option.value}
              onClick={() =>
                void navigate({
                  search: (previous) => ({ ...previous, show: option.value, page: undefined }),
                })
              }
            >
              {option.label}
            </Button>
          ))}
        </fieldset>
      </div>

      {!live.data ? (
        <Skeleton className="h-48 rounded-2xl" />
      ) : live.data.items.length === 0 ? (
        <p className="rounded-2xl border border-dashed px-4 py-8 text-center text-sm text-muted-foreground">
          {q || show
            ? "Nothing matches."
            : "No switcher is connected right now. Connected ones show up here."}
        </p>
      ) : (
        <ul className="flex flex-col overflow-hidden rounded-2xl border bg-card">
          {live.data.items.map((item) => {
            const tag = liveTag(item);
            return (
              <li
                key={item.switcher_id}
                className="flex flex-wrap items-center gap-x-4 gap-y-1 border-b border-divider px-4 py-3 last:border-b-0"
              >
                <StatusDot className={cn("size-2.5", tag.dot)} />
                <span className="flex min-w-0 flex-1 basis-56 flex-col">
                  <Link
                    to="/$workspace/s/$switcher/live"
                    params={{ workspace: item.workspace_slug, switcher: item.switcher_id }}
                    className="truncate text-sm font-semibold hover:underline"
                  >
                    {item.workspace_name}
                    <span className="font-normal text-muted-foreground">
                      {" "}
                      · {item.switcher_name}
                    </span>
                  </Link>
                  <span className="truncate text-xs text-muted-foreground">
                    {item.scene ? `On ${item.scene}` : "No scene"}
                    {item.streaming && item.stream_started_at
                      ? ` · live for ${formatUptime(item.stream_started_at, now)}`
                      : ""}
                  </span>
                </span>
                <span className="flex items-center gap-2">
                  {item.recording && <Badge variant="outline">REC</Badge>}
                  <span className={cn("font-mono text-xs font-semibold", tag.className)}>
                    {tag.tag}
                  </span>
                </span>
              </li>
            );
          })}
        </ul>
      )}
      {live.data && (
        <Pager
          page={page}
          total={live.data.total}
          perPage={live.data.per_page}
          onPage={(next) => void navigate({ search: (previous) => ({ ...previous, page: next }) })}
        />
      )}
    </Page>
  );
}

/** What is wrong with the streams that are live, if anything. */
function liveHint(counts: { live: number; low: number; offline: number } | undefined) {
  if (!counts || counts.live === 0) return undefined;
  const parts = [
    counts.low > 0 && `${counts.low} low`,
    counts.offline > 0 && `${counts.offline} without a feed`,
  ].filter(Boolean);
  return parts.length > 0 ? parts.join(" · ") : "all with a feed";
}
