import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import type { StreamServer } from "@/api/types";
import { formatKbps } from "@/lib/live-format";
import { kindInfo } from "@/lib/stream-server-kinds";
import { cn } from "@/lib/utils";
import type { Point } from "@/live/reducer";
import { BitrateChart } from "./bitrate-chart";
import { lookForState } from "./feed-state";

export function ServerCard({
  server,
  points,
  now,
  inUse,
  rank,
}: {
  server: StreamServer;
  points: Point[];
  now: number;
  inUse: boolean;
  rank?: number;
}) {
  const last = points[points.length - 1];
  const fresh = last && now - last.at < 5000;
  const look = lookForState(fresh ? last.state : "offline");

  return (
    <div
      className={cn(
        "server-row",
        "flex min-w-0 items-center gap-3 border-b border-divider py-3 last:border-b-0",
        !server.enabled && "opacity-60",
      )}
    >
      {rank != null && <span className="text-xs tabular-nums text-muted-foreground">P{rank}</span>}
      <span
        className={cn("size-1.5 shrink-0 rounded-full", server.enabled ? look.fill : "bg-idle")}
      />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-baseline gap-x-2">
          <span className="text-sm font-semibold">{server.name}</span>
          <span className="text-[11px] text-muted-foreground">
            {kindInfo(server.kind).label}
            {rank == null && ` · P${server.priority}`}
            {inUse && <> · {rank == null ? <Trans>in use</Trans> : <Trans>IN USE</Trans>}</>}
          </span>
        </div>
        <p className="mt-1 font-mono text-xs text-muted-foreground tabular">
          {!server.enabled ? (
            t`Disabled`
          ) : fresh ? (
            <>
              {formatKbps(last.bitrate)} <Trans>kbps</Trans> ·{" "}
              {last.rtt != null ? Math.round(last.rtt) : "—"} <Trans>ms</Trans>
              {rank == null && ` · ${look.label}`}
            </>
          ) : (
            t`No feed`
          )}
        </p>
      </div>
      <BitrateChart
        points={points}
        now={now}
        low={null}
        offline={null}
        thresholds={false}
        tone={look.text}
        height={28}
        className="w-20 shrink-0"
      />
    </div>
  );
}
