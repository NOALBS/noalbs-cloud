import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { ChevronRight, History } from "lucide-react";
import { streamsQuery, workspacePlanQuery } from "@/api/queries";
import type { StreamSummary } from "@/api/types";
import { StateBar } from "@/components/history/state-bar";
import { SwitcherPage } from "@/components/switcher-page";
import { Skeleton } from "@/components/ui/skeleton";
import { useSwitcher } from "@/hooks/use-switcher";
import { formatClock, formatDuration, groupByDay, stateShares } from "@/lib/history";

export const Route = createFileRoute("/_app/$workspace/s/$switcher/history/")({
  component: HistoryPage,
});

function HistoryPage() {
  const { workspace, switcher } = useSwitcher();
  const streams = useInfiniteQuery(streamsQuery(workspace.id, switcher.id));
  const plan = useQuery(workspacePlanQuery(workspace.id));
  const all = streams.data?.pages.flat() ?? [];
  const days = plan.data?.plan.history_days;

  return (
    <SwitcherPage
      title="History"
      description="Every stream from start to stop: how the feed held up, and what switched."
    >
      {streams.isPending && (
        <div className="flex flex-col gap-2.5">
          <Skeleton className="h-20" />
          <Skeleton className="h-20" />
          <Skeleton className="h-20" />
        </div>
      )}

      {streams.isSuccess && all.length === 0 && (
        <div className="flex flex-col items-center gap-3 rounded-2xl border border-dashed px-6 py-12 text-center">
          <History className="size-8 text-muted-foreground" />
          <p className="font-semibold">No streams yet</p>
          <p className="max-w-sm text-sm text-muted-foreground">
            When OBS goes live with the browser source connected, the stream is recorded here with
            its bitrate, RTT and scene switches.
          </p>
        </div>
      )}

      {groupByDay(all).map((group) => (
        <section key={group.key} aria-label={group.label} className="flex flex-col gap-2.5">
          <h2 className="text-sm font-semibold text-muted-foreground">{group.label}</h2>
          <ol className="flex flex-col overflow-hidden rounded-2xl border bg-card">
            {group.streams.map((stream) => (
              <li key={stream.id} className="border-b border-divider last:border-b-0">
                <StreamRow stream={stream} />
              </li>
            ))}
          </ol>
        </section>
      ))}

      {streams.hasNextPage && (
        <button
          type="button"
          onClick={() => void streams.fetchNextPage()}
          disabled={streams.isFetchingNextPage}
          className="min-h-11 self-center rounded-full border px-5 text-sm font-medium text-primary hover:bg-surface-2 disabled:opacity-50"
        >
          {streams.isFetchingNextPage ? "Loading…" : "Load older streams"}
        </button>
      )}

      {days != null && (
        <p className="text-center text-xs text-muted-foreground">
          The {plan.data?.plan.name} plan keeps {days} {days === 1 ? "day" : "days"} of history.{" "}
          <Link
            to="/$workspace/settings/plan"
            params={{ workspace: workspace.url_slug }}
            className="font-medium text-primary hover:underline"
          >
            See plans
          </Link>
        </p>
      )}
    </SwitcherPage>
  );
}

function StreamRow({ stream }: { stream: StreamSummary }) {
  const { workspace, switcher } = useSwitcher();
  const running = stream.ended_at == null;
  const start = Date.parse(stream.started_at);
  const shares = stateShares(stream);

  return (
    <Link
      to="/$workspace/s/$switcher/history/$stream"
      params={{ workspace: workspace.url_slug, switcher: switcher.id, stream: stream.id }}
      className="flex items-center gap-4 px-4 py-3.5 hover:bg-surface-2 md:px-5"
    >
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <span className="font-mono text-base font-semibold tabular">
            {formatClock(start)} –{" "}
            {running ? "now" : formatClock(Date.parse(stream.ended_at ?? ""))}
          </span>
          <span className="text-sm text-muted-foreground">
            {formatDuration(stream.duration_seconds)}
          </span>
          {running && (
            <span className="flex items-center gap-1.5 rounded-full bg-live/12 px-2 py-0.5 text-[11px] font-bold tracking-wider text-live">
              <span className="size-1.5 animate-pulse rounded-full bg-live" />
              LIVE NOW
            </span>
          )}
        </div>
        <StateBar seconds={stream} className="max-w-md" />
      </div>
      <div className="hidden shrink-0 flex-col items-end gap-0.5 text-right sm:flex">
        <span className="font-mono text-sm font-semibold text-live tabular">
          {Math.round(shares.live * 100)}% live
        </span>
        <span className="text-xs text-muted-foreground">
          {stream.auto_switches === 1 ? "1 auto switch" : `${stream.auto_switches} auto switches`}
        </span>
      </div>
      <ChevronRight className="size-4 shrink-0 text-muted-foreground" />
    </Link>
  );
}
