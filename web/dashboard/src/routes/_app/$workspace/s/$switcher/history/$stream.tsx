import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { ChevronLeft, Trash2, ZoomOut } from "lucide-react";
import { type ReactNode, useMemo, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys, type Range, streamQuery } from "@/api/queries";
import type { SeriesPoint, StreamDetail, StreamSource, SwitcherEvent } from "@/api/types";
import {
  HistoryChart,
  type Marker,
  StateStrip,
  TimeAxis,
} from "@/components/history/history-chart";
import { MarkersPanel } from "@/components/history/markers-panel";
import { RawExplorer } from "@/components/history/raw-explorer";
import { StateBar } from "@/components/history/state-bar";
import { lookForState } from "@/components/live/feed-state";
import { SwitcherPage } from "@/components/switcher-page";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useSwitcher } from "@/hooks/use-switcher";
import { errorMessage } from "@/lib/errors";
import { formatClock, formatDuration, pointAt, worstState } from "@/lib/history";
import { describeEvent, formatKbps } from "@/lib/live-format";
import { currentLocale } from "@/lib/locale";
import { kindInfo } from "@/lib/stream-server-kinds";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/_app/$workspace/s/$switcher/history/$stream")({
  component: StreamPage,
});

type Tab = "overview" | "markers" | "raw";

/** A series point with its time as a number, as the charts use it. */
type Point = Omit<SeriesPoint, "at"> & { at: number };

function StreamPage() {
  const { workspace, switcher, canManage, canReadSecrets } = useSwitcher();
  const { stream: streamId } = Route.useParams();
  const [range, setRange] = useState<Range | undefined>();
  const [hoverAt, setHoverAt] = useState<number | null>(null);
  const [tab, setTab] = useState<Tab>("overview");
  const detail = useQuery({
    ...streamQuery(workspace.id, switcher.id, streamId, range),
    placeholderData: keepPreviousData,
  });
  const params = { workspace: workspace.url_slug, switcher: switcher.id };

  const back = (
    <Link
      to="/$workspace/s/$switcher/history"
      params={params}
      className="-mb-2 flex min-h-11 items-center gap-1 self-start text-sm font-medium text-primary"
    >
      <ChevronLeft className="size-4" />
      History
    </Link>
  );

  if (detail.isPending) {
    return (
      <SwitcherPage title="Stream" back={back}>
        <Skeleton className="h-24" />
        <Skeleton className="h-64" />
      </SwitcherPage>
    );
  }
  if (detail.isError) {
    return (
      <SwitcherPage title="Stream" back={back}>
        <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
          {errorMessage(detail.error)} It may be older than your plan keeps.
        </p>
      </SwitcherPage>
    );
  }

  const data = detail.data;
  const stream = data.stream;
  const from = Date.parse(data.from);
  const to = Date.parse(data.to);
  const running = stream.ended_at == null;
  const zoom = (start: number, end: number) => {
    setRange({ from: new Date(start).toISOString(), to: new Date(end).toISOString() });
    setHoverAt(null);
  };

  return (
    <SwitcherPage
      back={back}
      title={new Date(stream.started_at).toLocaleDateString(currentLocale(), {
        weekday: "long",
        day: "numeric",
        month: "long",
      })}
      description={`${formatClock(Date.parse(stream.started_at))} – ${
        running ? "now" : formatClock(Date.parse(stream.ended_at ?? ""))
      } · ${formatDuration(stream.duration_seconds)}`}
      actions={
        <>
          {running && (
            <span className="flex h-8 items-center gap-1.5 rounded-full bg-live/12 px-3 text-xs font-bold tracking-wider text-live">
              <span className="size-1.5 animate-pulse rounded-full bg-live" />
              LIVE NOW
            </span>
          )}
          {canManage && <DeleteStream streamId={stream.id} />}
        </>
      }
    >
      <Summary data={data} />

      <div className="flex flex-wrap items-center justify-between gap-3">
        <div
          role="tablist"
          aria-label="View"
          className="flex rounded-full border bg-card p-1 text-sm font-medium"
        >
          <TabButton active={tab === "overview"} onClick={() => setTab("overview")}>
            Overview
          </TabButton>
          <TabButton active={tab === "markers"} onClick={() => setTab("markers")}>
            Markers
          </TabButton>
          {canReadSecrets && (
            <TabButton active={tab === "raw"} onClick={() => setTab("raw")}>
              Raw stats
            </TabButton>
          )}
        </div>
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          {tab !== "markers" &&
            (range ? (
              <>
                <span className="font-mono tabular">
                  {formatClock(from, true)} – {formatClock(to, true)}
                </span>
                <Button size="sm" variant="outline" onClick={() => setRange(undefined)}>
                  <ZoomOut />
                  Whole stream
                </Button>
              </>
            ) : (
              <span className="hidden md:inline">Drag across a chart to zoom in</span>
            ))}
          {detail.isFetching && <span className="sr-only">Loading…</span>}
        </div>
      </div>

      {tab === "overview" ? (
        <Overview
          data={data}
          from={from}
          to={to}
          hoverAt={hoverAt}
          onHover={setHoverAt}
          onZoom={zoom}
        />
      ) : tab === "markers" ? (
        <MarkersPanel
          workspaceId={workspace.id}
          switcherId={switcher.id}
          streamId={stream.id}
          startedAt={stream.started_at}
        />
      ) : (
        <RawExplorer
          workspaceId={workspace.id}
          switcherId={switcher.id}
          streamId={stream.id}
          sources={data.sources}
          range={range}
          from={from}
          to={to}
          hoverAt={hoverAt}
          onHover={setHoverAt}
          onZoom={zoom}
        />
      )}
    </SwitcherPage>
  );
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={onClick}
      className={cn(
        "min-h-8 rounded-full px-4 text-muted-foreground",
        active && "bg-primary-soft text-primary-soft-foreground",
      )}
    >
      {children}
    </button>
  );
}

/** Each server's points with times as numbers, in source order. */
function useServerPoints(data: StreamDetail) {
  return useMemo(() => {
    const byServer = new Map(
      data.series.map((series) => [
        series.server_id,
        series.points.map((point): Point => ({ ...point, at: Date.parse(point.at) })),
      ]),
    );
    const known = new Set(data.sources.map((source) => source.server_id));
    const sources: StreamSource[] = [
      ...data.sources,
      // Stats from a server whose record went missing still show.
      ...data.series
        .filter((series) => !known.has(series.server_id))
        .map((series) => ({
          server_id: series.server_id,
          name: "Removed server",
          kind: "belabox" as const,
        })),
    ];
    return sources.map((source) => ({ source, points: byServer.get(source.server_id) ?? [] }));
  }, [data]);
}

function Summary({ data }: { data: StreamDetail }) {
  const stream = data.stream;
  const servers = useServerPoints(data);
  // The server that carried the stream: the one with the most feed.
  const main = [...servers].sort(
    (a, b) =>
      b.points.reduce((sum, p) => sum + p.live_seconds + p.low_seconds, 0) -
      a.points.reduce((sum, p) => sum + p.live_seconds + p.low_seconds, 0),
  )[0];
  const fed = main?.points.filter((p) => p.live_seconds + p.low_seconds > 0) ?? [];
  const average = fed.length > 0 ? fed.reduce((sum, p) => sum + p.bitrate, 0) / fed.length : null;
  const rtts = fed.flatMap((p) => (p.rtt != null ? [p.rtt] : []));
  const rtt = rtts.length > 0 ? rtts.reduce((sum, value) => sum + value, 0) / rtts.length : null;

  return (
    <section aria-label="Summary" className="flex flex-col gap-4 rounded-2xl border bg-card p-5">
      <div className="grid grid-cols-2 gap-x-4 gap-y-5 sm:grid-cols-3 lg:grid-cols-6">
        <Tile label="Duration" value={formatDuration(stream.duration_seconds)} />
        <Tile label="LIVE" value={formatDuration(stream.live_seconds)} tone="text-live" />
        <Tile label="LOW" value={formatDuration(stream.low_seconds)} tone="text-low" />
        <Tile label="BRB" value={formatDuration(stream.offline_seconds)} tone="text-offline" />
        <Tile label="Auto switches" value={String(stream.auto_switches)} />
        <Tile
          label={main && servers.length > 1 ? `Avg · ${main.source.name}` : "Avg bitrate"}
          value={average != null ? `${formatKbps(Math.round(average))}` : "—"}
          unit={average != null ? "kbps" : undefined}
          hint={rtt != null ? `RTT ${Math.round(rtt)} ms` : undefined}
        />
      </div>
      <StateBar seconds={stream} legend />
    </section>
  );
}

function Tile({
  label,
  value,
  unit,
  hint,
  tone,
}: {
  label: string;
  value: string;
  unit?: string;
  hint?: string;
  tone?: string;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <span className="truncate text-xs tracking-wider text-muted-foreground uppercase">
        {label}
      </span>
      <span className={cn("font-mono text-2xl leading-none font-semibold tabular", tone)}>
        {value}
        {unit && <span className="text-sm font-normal text-muted-foreground"> {unit}</span>}
      </span>
      {hint && <span className="font-mono text-xs text-muted-foreground tabular">{hint}</span>}
    </div>
  );
}

function Overview({
  data,
  from,
  to,
  hoverAt,
  onHover,
  onZoom,
}: {
  data: StreamDetail;
  from: number;
  to: number;
  hoverAt: number | null;
  onHover: (at: number | null) => void;
  onZoom: (from: number, to: number) => void;
}) {
  const servers = useServerPoints(data);
  const [pinnedEvent, setPinnedEvent] = useState<string | null>(null);
  const switches = data.events.filter((event) => event.kind === "scene_switched");
  const markers: Marker[] = switches.map((event) => ({
    at: Date.parse(event.created_at),
    highlighted: event.id === pinnedEvent,
  }));

  return (
    <div className="grid grid-cols-1 gap-4 lg:grid-cols-12">
      <div className="flex min-w-0 flex-col gap-4 lg:col-span-8">
        {servers.length === 0 && (
          <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
            No stream server stats were recorded for this stream.
          </p>
        )}
        {servers.map(({ source, points }) => (
          <ServerHistory
            key={source.server_id}
            source={source}
            points={points}
            from={from}
            to={to}
            stepMs={data.step_seconds * 1000}
            markers={markers}
            hoverAt={hoverAt}
            onHover={onHover}
            onZoom={onZoom}
          />
        ))}
      </div>
      <div className="lg:col-span-4">
        <StreamActivity
          events={data.events}
          pinned={pinnedEvent}
          onPick={(event) => {
            setPinnedEvent(event?.id ?? null);
            onHover(event ? Date.parse(event.created_at) : null);
          }}
        />
      </div>
    </div>
  );
}

function ServerHistory({
  source,
  points,
  from,
  to,
  stepMs,
  markers,
  hoverAt,
  onHover,
  onZoom,
}: {
  source: StreamSource;
  points: Point[];
  from: number;
  to: number;
  stepMs: number;
  markers: Marker[];
  hoverAt: number | null;
  onHover: (at: number | null) => void;
  onZoom: (from: number, to: number) => void;
}) {
  const { switcher } = useSwitcher();
  const chart = points.map((point) => ({
    at: point.at,
    value: point.bitrate,
    min: point.bitrate_min,
    max: point.bitrate_max,
  }));
  const rtt = points.map((point) => ({ at: point.at, value: point.rtt ?? null }));
  const hasRtt = rtt.some((point) => point.value != null);
  const states = points.map((point) => ({ at: point.at, state: worstState(point) }));
  const hoveredAt = hoverAt != null ? pointAt(points, hoverAt, stepMs) : undefined;
  const look = hoveredAt ? lookForState(worstState(hoveredAt)) : undefined;

  return (
    <section
      aria-label={source.name}
      className="flex flex-col gap-3 rounded-2xl border bg-card p-4 md:p-5"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="flex min-w-0 items-center gap-2">
          <span className="truncate font-semibold">{source.name}</span>
          <Badge className="bg-primary-soft text-[10px] tracking-wider text-primary-soft-foreground uppercase">
            {kindInfo(source.kind).label}
          </Badge>
        </span>
        <div className="flex flex-wrap gap-3 text-xs text-muted-foreground">
          {switcher.triggers.low != null && (
            <span className="flex items-center gap-1.5">
              <span className="w-3.5 border-t border-dashed border-low" />
              Low {formatKbps(switcher.triggers.low)}
            </span>
          )}
          {switcher.triggers.offline != null && (
            <span className="flex items-center gap-1.5">
              <span className="w-3.5 border-t border-dashed border-offline" />
              Offline {formatKbps(switcher.triggers.offline)}
            </span>
          )}
        </div>
      </div>

      <Readout>
        {hoveredAt ? (
          <>
            <span className="text-muted-foreground">{formatClock(hoveredAt.at, true)}</span>
            <span>
              <strong className="text-base">{formatKbps(Math.round(hoveredAt.bitrate))}</strong>{" "}
              kbps
            </span>
            <span className="text-muted-foreground">
              {formatKbps(hoveredAt.bitrate_min)}–{formatKbps(hoveredAt.bitrate_max)}
            </span>
            {hoveredAt.rtt != null && <span>RTT {Math.round(hoveredAt.rtt)} ms</span>}
            {look && <span className={cn("font-semibold", look.text)}>{look.label}</span>}
          </>
        ) : (
          <span className="text-muted-foreground">
            {points.length === 0
              ? "No stats from this server."
              : "Hover or tap a chart for the numbers at that moment."}
          </span>
        )}
      </Readout>

      <HistoryChart
        from={from}
        to={to}
        stepMs={stepMs}
        lines={[{ points: chart, tone: "text-primary", band: true }]}
        thresholds={[
          ...(switcher.triggers.low != null
            ? [{ value: switcher.triggers.low, className: "stroke-low" }]
            : []),
          ...(switcher.triggers.offline != null
            ? [{ value: switcher.triggers.offline, className: "stroke-offline" }]
            : []),
        ]}
        markers={markers}
        minMax={2000}
        hoverAt={hoverAt}
        onHover={onHover}
        onZoom={onZoom}
        formatValue={(value) => formatKbps(Math.round(value))}
      />
      <StateStrip from={from} to={to} stepMs={stepMs} points={states} />

      {hasRtt && (
        <div className="flex flex-col gap-1.5">
          <span className="text-xs tracking-wider text-muted-foreground uppercase">RTT (ms)</span>
          <HistoryChart
            from={from}
            to={to}
            stepMs={stepMs}
            lines={[{ points: rtt, tone: "text-subtle-foreground" }]}
            thresholds={[
              ...(switcher.triggers.rtt != null
                ? [{ value: switcher.triggers.rtt, className: "stroke-low" }]
                : []),
              ...(switcher.triggers.rtt_offline != null
                ? [{ value: switcher.triggers.rtt_offline, className: "stroke-offline" }]
                : []),
            ]}
            markers={markers}
            minMax={100}
            hoverAt={hoverAt}
            onHover={onHover}
            onZoom={onZoom}
            formatValue={(value) => String(Math.round(value))}
            height={90}
          />
        </div>
      )}
      <TimeAxis from={from} to={to} />
    </section>
  );
}

function Readout({ children }: { children: ReactNode }) {
  return (
    <div className="flex min-h-6 flex-wrap items-baseline gap-x-3 gap-y-1 font-mono text-xs tabular">
      {children}
    </div>
  );
}

function StreamActivity({
  events,
  pinned,
  onPick,
}: {
  events: SwitcherEvent[];
  pinned: string | null;
  onPick: (event: SwitcherEvent | null) => void;
}) {
  return (
    <section aria-label="Activity" className="flex flex-col gap-2.5">
      <h2 className="font-semibold">Activity</h2>
      <div className="flex max-h-[32rem] flex-col overflow-y-auto overscroll-contain rounded-2xl border bg-card">
        {events.length === 0 && (
          <p className="px-4 py-6 text-sm text-muted-foreground">
            Nothing happened during this stream.
          </p>
        )}
        <ol>
          {events.map((event) => {
            const { title, detail } = describeEvent(event);
            const active = event.id === pinned;
            return (
              <li key={event.id} className="border-b border-divider last:border-b-0">
                <button
                  type="button"
                  onClick={() => onPick(active ? null : event)}
                  aria-pressed={active}
                  className={cn(
                    "flex w-full flex-col gap-0.5 px-4 py-2.5 text-left hover:bg-surface-2",
                    active && "bg-primary-soft",
                  )}
                >
                  <span className="flex items-baseline justify-between gap-2">
                    <span className="text-sm font-semibold">{title}</span>
                    <time
                      dateTime={event.created_at}
                      className="font-mono text-[11px] text-muted-foreground tabular"
                    >
                      {formatClock(Date.parse(event.created_at), true)}
                    </time>
                  </span>
                  <span className="text-xs text-muted-foreground">{detail}</span>
                </button>
              </li>
            );
          })}
        </ol>
      </div>
    </section>
  );
}

function DeleteStream({ streamId }: { streamId: string }) {
  const { workspace, switcher, path } = useSwitcher();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const remove = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE("/workspaces/{workspace_id}/switchers/{switcher_id}/streams/{stream_id}", {
          params: { path: { ...path, stream_id: streamId } },
        }),
      ),
    onSuccess: async () => {
      await navigate({
        to: "/$workspace/s/$switcher/history",
        params: { workspace: workspace.url_slug, switcher: switcher.id },
      });
      await queryClient.invalidateQueries({
        queryKey: queryKeys.streams(workspace.id, switcher.id),
      });
      toast.success("Stream deleted");
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button size="sm" variant="outline" disabled={remove.isPending}>
          <Trash2 />
          Delete
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete this stream?</AlertDialogTitle>
          <AlertDialogDescription>
            Its stats and raw stats are removed from the history for everyone. The activity log
            keeps its events.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={() => remove.mutate()}>
            Delete
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
