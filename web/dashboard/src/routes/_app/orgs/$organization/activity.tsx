import { useInfiniteQuery } from "@tanstack/react-query";
import { createFileRoute, notFound, redirect, useNavigate } from "@tanstack/react-router";
import { X } from "lucide-react";
import { useCallback } from "react";
import { ApiRequestError } from "@/api/client";
import { organizationActivityQuery, organizationBySlugQuery } from "@/api/queries";
import { SearchBox } from "@/components/admin/list-controls";
import { ActivityList } from "@/components/orgs/activity-list";
import { AuditLog } from "@/components/orgs/audit-log";
import { Page } from "@/components/page";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useOrganization } from "@/hooks/use-organization";
import { ACTIVITY_PRESETS, parseActivitySearch } from "@/lib/organizations";

export const Route = createFileRoute("/_app/orgs/$organization/activity")({
  validateSearch: parseActivitySearch,
  // Admins only: the team's members see each client's own activity.
  beforeLoad: async ({ context: { queryClient }, params }) => {
    const organization = await queryClient
      .ensureQueryData(organizationBySlugQuery(params.organization))
      .catch((error) => {
        if (error instanceof ApiRequestError && error.status === 404) throw notFound();
        throw error;
      });
    if (organization.role !== "admin") {
      throw redirect({
        to: "/orgs/$organization/clients",
        params: { organization: params.organization },
      });
    }
  },
  component: ActivityPage,
});

function ActivityPage() {
  const { organization } = useOrganization();
  const { q = "", show, client, view } = Route.useSearch();
  const navigate = useNavigate({ from: Route.fullPath });
  const kinds = ACTIVITY_PRESETS.find((preset) => preset.value === show)?.kinds;
  const activity = useInfiniteQuery(
    organizationActivityQuery(organization.id, { q, kinds, client }),
  );
  const events = activity.data?.pages.flat() ?? [];
  const search = useCallback(
    (value: string) =>
      void navigate({
        search: (previous) => ({ ...previous, q: value || undefined }),
        replace: true,
      }),
    [navigate],
  );

  return (
    <Page
      title="Activity"
      description={
        view === "changes"
          ? "What your team, your customers and NOALBS changed in the organization. Kept for a year."
          : "What happened on your clients’ switchers, and who did it: the automatic switcher, your team, a customer or chat."
      }
    >
      <fieldset className="flex gap-1" aria-label="View">
        {[
          { value: undefined, label: "Switchers" },
          { value: "changes" as const, label: "Changes" },
        ].map((tab) => (
          <Button
            key={tab.label}
            size="sm"
            variant={view === tab.value ? "secondary" : "ghost"}
            aria-pressed={view === tab.value}
            onClick={() =>
              void navigate({
                search: tab.value ? { view: tab.value } : {},
              })
            }
          >
            {tab.label}
          </Button>
        ))}
      </fieldset>

      {view === "changes" ? (
        <AuditLog organization={organization} />
      ) : (
        <>
          <div className="flex flex-wrap items-center gap-2">
            <SearchBox value={q} onSearch={search} placeholder="Search by client or switcher" />
            <fieldset className="flex flex-wrap gap-1" aria-label="Show">
              {ACTIVITY_PRESETS.map((preset) => {
                const active = (show ?? "all") === preset.value;
                return (
                  <Button
                    key={preset.value}
                    size="sm"
                    variant={active ? "secondary" : "ghost"}
                    aria-pressed={active}
                    onClick={() =>
                      void navigate({
                        search: (previous) => ({
                          ...previous,
                          show: preset.value === "all" ? undefined : preset.value,
                        }),
                      })
                    }
                  >
                    {preset.label}
                  </Button>
                );
              })}
            </fieldset>
            {client && (
              <Button
                size="sm"
                variant="outline"
                onClick={() =>
                  void navigate({ search: (previous) => ({ ...previous, client: undefined }) })
                }
              >
                One client only
                <X />
              </Button>
            )}
          </div>

          {activity.isPending ? (
            <Skeleton className="h-64 rounded-2xl" />
          ) : events.length === 0 ? (
            <p className="rounded-2xl border border-dashed px-4 py-8 text-center text-sm text-muted-foreground">
              {q || show || client
                ? "Nothing matches."
                : "Nothing yet. Scene switches and stream changes of your clients show up here."}
            </p>
          ) : (
            <ActivityList events={events} />
          )}
          {activity.hasNextPage && (
            <Button
              variant="outline"
              className="self-center"
              disabled={activity.isFetchingNextPage}
              onClick={() => void activity.fetchNextPage()}
            >
              {activity.isFetchingNextPage ? "Loading…" : "Load older"}
            </Button>
          )}
        </>
      )}
    </Page>
  );
}
