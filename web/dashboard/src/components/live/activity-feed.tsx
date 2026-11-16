import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useInfiniteQuery } from "@tanstack/react-query";
import { eventsQuery } from "@/api/queries";
import type { SwitcherEvent } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { describeEvent } from "@/lib/live-format";
import { currentLocale } from "@/lib/locale";

/**
 * Recent activity: the saved backlog plus events arriving live, in a box that
 * scrolls on its own, with older pages loaded on request.
 */
export function ActivityFeed({
  workspaceId,
  switcherId,
  live,
}: {
  workspaceId: string;
  switcherId: string;
  live: SwitcherEvent[];
}) {
  const backlog = useInfiniteQuery(eventsQuery(workspaceId, switcherId));
  const seen = new Set(live.map((event) => event.id));
  const saved = backlog.data?.pages.flat() ?? [];
  const events = [...live, ...saved.filter((event) => !seen.has(event.id))];

  return (
    <section aria-label={t`Activity`} className="flex flex-col gap-2.5">
      <div className="flex items-center justify-between gap-2">
        <h2 className="font-semibold">
          <Trans>Activity</Trans>
        </h2>
        <span className="text-xs text-muted-foreground">
          <Trans>This stream</Trans>
        </span>
      </div>
      <div className="flex max-h-96 flex-col overflow-y-auto overscroll-contain">
        {backlog.isPending && <Skeleton className="m-4 h-24" />}
        {backlog.isError && (
          <p className="py-3 text-sm text-muted-foreground">
            <Trans>Couldn’t load activity.</Trans>{" "}
            <Button
              variant="ghost"
              type="button"
              className="text-primary hover:underline"
              onClick={() => void backlog.refetch()}
            >
              <Trans>Retry</Trans>
            </Button>
          </p>
        )}
        {!backlog.isPending && !backlog.isError && events.length === 0 && (
          <p className="px-4 py-6 text-sm text-muted-foreground">
            <Trans>Nothing yet. Scene switches and stream changes show up here.</Trans>
          </p>
        )}
        <ol>
          {events.map((event) => {
            const { title, detail } = describeEvent(event);
            return (
              <li
                key={event.id}
                className="grid grid-cols-[3.75rem_1fr] items-baseline gap-x-3 border-b border-divider py-2.5 last:border-b-0"
              >
                <time
                  dateTime={event.created_at}
                  className="font-mono text-[11px] text-muted-foreground tabular"
                >
                  {new Date(event.created_at).toLocaleTimeString(currentLocale(), {
                    hourCycle: "h23",
                  })}
                </time>
                <span className="min-w-0">
                  <span className="block text-sm font-semibold">{title}</span>
                  <span className="block text-xs text-muted-foreground">{detail}</span>
                </span>
              </li>
            );
          })}
        </ol>
        {backlog.hasNextPage && (
          <Button
            variant="ghost"
            type="button"
            onClick={() => void backlog.fetchNextPage()}
            disabled={backlog.isFetchingNextPage}
            className="min-h-11 shrink-0 border-t border-divider text-sm font-medium text-primary hover:bg-surface-2 disabled:opacity-50"
          >
            {backlog.isFetchingNextPage ? t`Loading…` : t`Load older`}
          </Button>
        )}
      </div>
    </section>
  );
}
