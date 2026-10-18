import { useInfiniteQuery } from "@tanstack/react-query";
import { eventsQuery } from "@/api/queries";
import type { SwitcherEvent } from "@/api/types";
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
    <section aria-label="Activity" className="flex flex-col gap-2.5">
      <h2 className="font-semibold">Activity</h2>
      <div className="flex max-h-96 flex-col overflow-y-auto overscroll-contain rounded-2xl border bg-card lg:max-h-[27rem]">
        {backlog.isPending && <Skeleton className="m-4 h-24" />}
        {!backlog.isPending && events.length === 0 && (
          <p className="px-4 py-6 text-sm text-muted-foreground">
            Nothing yet. Scene switches and stream changes show up here.
          </p>
        )}
        <ol>
          {events.map((event) => {
            const { title, detail } = describeEvent(event);
            return (
              <li
                key={event.id}
                className="flex flex-col gap-0.5 border-b border-divider px-4 py-2.5 last:border-b-0"
              >
                <span className="flex items-baseline justify-between gap-2">
                  <span className="text-sm font-semibold">{title}</span>
                  <time
                    dateTime={event.created_at}
                    className="font-mono text-[11px] text-muted-foreground tabular"
                  >
                    {new Date(event.created_at).toLocaleTimeString(currentLocale(), {
                      hourCycle: "h23",
                    })}
                  </time>
                </span>
                <span className="text-xs text-muted-foreground">{detail}</span>
              </li>
            );
          })}
        </ol>
        {backlog.hasNextPage && (
          <button
            type="button"
            onClick={() => void backlog.fetchNextPage()}
            disabled={backlog.isFetchingNextPage}
            className="min-h-11 shrink-0 border-t border-divider text-sm font-medium text-primary hover:bg-surface-2 disabled:opacity-50"
          >
            {backlog.isFetchingNextPage ? "Loading…" : "Load older"}
          </button>
        )}
      </div>
    </section>
  );
}
