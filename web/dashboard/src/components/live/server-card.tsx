import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import type { StreamServer } from "@/api/types";
import { formatKbps } from "@/lib/live-format";
import { kindInfo } from "@/lib/stream-server-kinds";
import { cn } from "@/lib/utils";
import type { Point } from "@/live/reducer";
import { BitrateChart } from "./bitrate-chart";
import { lookForState } from "./feed-state";

const chip =
  "rounded-md border bg-muted px-1.5 py-px text-[10px] leading-4 font-medium text-muted-foreground";

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
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
          <span className="truncate text-sm font-semibold">{server.name}</span>
          <span className="flex items-center gap-1.5">
            <span className={chip}>{kindInfo(server.kind).label}</span>
            <span className={cn(chip, "font-mono tabular")}>P{rank ?? server.priority}</span>
            {inUse && (
              <span className={cn(chip, "border-primary/40 bg-primary/10 text-primary")}>
                <Trans>In use</Trans>
              </span>
            )}
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
