import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import type { StreamServer, SwitcherEvent, SwitcherStatus } from "@/api/types";
import { Skeleton } from "@/components/ui/skeleton";
import { formatOffset } from "@/lib/history";
import { describeEvent, formatKbps, formatUptime } from "@/lib/live-format";
import { cn } from "@/lib/utils";
import { type LiveState as Feed, latest, type Point } from "@/live/reducer";
import { BitrateChart } from "./bitrate-chart";
import { lookFor } from "./feed-state";
import type { LiveState } from "./live-state";

export function lastFreshAt(history: Record<string, Point[]>): number | null {
  const times = Object.values(history).flatMap((points) =>
    points
      .filter((point) => point.state === "live" || point.state === "low")
      .map((point) => point.at),
  );
  return times.length ? Math.max(...times) : null;
}

export function LiveHero({
  state,
  status,
  servers,
  activeServer,
  live,
  now,
  low,
  offline,
  scenesSet,
  narrow,
  paused,
}: {
  state: LiveState;
  status: SwitcherStatus | null | undefined;
  servers: StreamServer[];
  activeServer: StreamServer | undefined;
  live: Feed;
  now: number;
  low: number | null | undefined;
  offline: number | null | undefined;
  scenesSet: boolean;
  narrow: boolean;
  paused: boolean;
}) {
  const look = lookFor(status);
  const sample = activeServer && status?.obs_connected ? latest(live, activeServer.id) : undefined;
  const offlineObs = status === null || (status != null && !status.obs_connected);
  const noFeed = Boolean(status?.streaming && status.state === "offline");
  const notStreaming = Boolean(status?.obs_connected && !status.streaming);
  const lastFresh = noFeed ? lastFreshAt(live.history) : null;
  const sceneChange = live.events.find(
    (event): event is SwitcherEvent & { kind: "scene_switched" } =>
      event.kind === "scene_switched" && event.data.to === status?.current_scene?.name,
  );
  const why = paused
    ? t`Not switching while paused`
    : offlineObs
      ? t`Unknown while OBS is offline`
      : !status?.auto_switch
        ? t`Auto switch is off`
        : sceneChange
          ? describeEvent(sceneChange).detail
          : null;
  const label = paused
    ? t`Paused`
    : offlineObs
      ? t`Offline`
      : notStreaming
        ? t`Not streaming`
        : noFeed
          ? "BRB"
          : status?.state === "low"
            ? "LOW"
            : "LIVE";
  if (state === "connecting")
    return (
      <div role="status" className="space-y-4">
        <p className="text-sm text-muted-foreground">
          <Trans>Connecting…</Trans>
        </p>
        <Skeleton className="h-14 w-48" />
        <Skeleton className="h-28 w-full" />
      </div>
    );
  return (
    <>
      <div className="mb-4 flex items-center gap-3">
        <span
          role="status"
          className={cn(
            "rounded-md px-2 py-0.5 text-xs font-semibold",
            paused
              ? "bg-low/10 text-low"
              : notStreaming
                ? "bg-muted text-muted-foreground"
                : look.soft,
            !paused && !notStreaming && look.text,
          )}
        >
          {label}
        </span>
        {status?.streaming && status.stream_started_at && (
          <span className="font-display text-xs text-muted-foreground tabular-nums">
            {formatUptime(status.stream_started_at, now)}
          </span>
        )}
      </div>
      <div className={cn("flex gap-5", narrow ? "flex-col" : "flex-col lg:flex-row")}>
        <div className={cn("flex min-w-0 flex-col gap-4", !narrow && "lg:w-[38%] lg:shrink-0")}>
          {offlineObs ? (
            <>
              <h3 className="font-display text-3xl font-bold">
                <Trans>OBS is offline</Trans>
              </h3>
              <p className="text-xs text-muted-foreground">
                {status?.pc
                  ? t`OBS isn’t connected to Connect.`
                  : t`NOALBS needs OBS open with the browser source active.`}
              </p>
            </>
          ) : notStreaming ? (
            <>
              <h3 className="font-display text-3xl font-bold">
                <Trans>No feed yet</Trans>
              </h3>
              <ul className="space-y-1 text-xs">
                <Check done={Boolean(status?.obs_connected)} label={t`OBS connected`} />
                <Check
                  done={servers.some((server) => {
                    const point = latest(live, server.id);
                    return (
                      server.enabled && point != null && now - point.at < 5000 && point.bitrate > 0
                    );
                  })}
                  label={t`Server receiving video`}
                />
                <Check done={scenesSet} label={t`LIVE, LOW and BRB scenes set`} />
              </ul>
            </>
          ) : noFeed ? (
            <h3 className="font-display text-3xl font-bold">
              <Trans>No feed</Trans>
              {lastFresh != null && (
                <span className="ml-2 text-base font-normal text-muted-foreground">
                  <Trans>for {formatOffset((now - lastFresh) / 1000)}</Trans>
                </span>
              )}
            </h3>
          ) : (
            <div>
              <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1 font-display tabular-nums">
                <span className="text-5xl font-bold">
                  {sample ? formatKbps(sample.bitrate) : "—"}
                </span>
                <span className="text-xs text-muted-foreground">
                  <Trans>kbps</Trans>
                </span>
                <span className="ml-2 text-lg font-semibold">
                  {sample?.rtt != null ? Math.round(sample.rtt) : "—"}
                </span>
                <span className="text-xs text-muted-foreground">
                  <Trans>ms RTT</Trans>
                </span>
              </div>
              {activeServer && (
                <p className="mt-2 text-xs text-muted-foreground">
                  {activeServer.name}
                  {activeServer.id === status?.active_server_id && (
                    <>
                      {" "}
                      · <Trans>in use</Trans>
                    </>
                  )}
                </p>
              )}
            </div>
          )}
          <div className="mt-auto space-y-1">
            <p className="text-xs text-muted-foreground">{notStreaming ? t`In OBS` : t`On air`}</p>
            <p className="text-sm font-semibold">{status?.current_scene?.name ?? "—"}</p>
            {why && <p className="text-xs text-muted-foreground">{why}</p>}
          </div>
        </div>
        <div className="flex min-w-0 flex-1 flex-col justify-center">
          {offlineObs || notStreaming ? (
            <p className="flex min-h-28 items-center justify-center text-center text-xs text-muted-foreground">
              <Trans>The bitrate shows here once a server receives video.</Trans>
            </p>
          ) : (
            <>
              <BitrateChart
                series={servers.map((server) => ({
                  id: server.id,
                  points: live.history[server.id] ?? [],
                  tone:
                    server.id === status?.active_server_id ? look.text : "text-muted-foreground",
                  filled: server.id === status?.active_server_id,
                }))}
                now={now}
                low={low}
                offline={offline}
                tone={look.text}
              />
              <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
                {servers.map((server) => {
                  const point = latest(live, server.id);
                  return (
                    <span key={server.id} className="flex items-center gap-1.5">
                      <span
                        className={cn(
                          "inline-block w-3 border-t-2",
                          server.id === status?.active_server_id
                            ? look.text
                            : "text-muted-foreground",
                        )}
                      />
                      {server.name} {point ? formatKbps(point.bitrate) : "—"}
                    </span>
                  );
                })}
              </div>
            </>
          )}
          <p className="mt-3 text-right text-[10px] text-muted-foreground">
            {low != null && <>LOW {formatKbps(low)} · </>}
            {offline != null && <>BRB {formatKbps(offline)} · </>}
            <Trans>last 2 min</Trans>
          </p>
        </div>
      </div>
    </>
  );
}
function Check({ done, label }: { done: boolean; label: string }) {
  return (
    <li className="flex gap-2">
      <span role="img" aria-label={done ? t`Done` : t`Not done`} className="text-muted-foreground">
        {done ? "✓" : "—"}
      </span>
      {label}
    </li>
  );
}
