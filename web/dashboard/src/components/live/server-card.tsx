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
}: {
  server: StreamServer;
  points: Point[];
  now: number;
  inUse: boolean;
}) {
  const last = points.at(-1);
  const fresh = last != null && now - last.at < 5000;
  const look = lookForState(fresh ? last.state : "offline");
  return (
    <div className={cn("flex min-w-0 items-center gap-3 py-3", !server.enabled && "opacity-60")}>
      <span
        className={cn("size-1.5 shrink-0 rounded-full", server.enabled ? look.fill : "bg-idle")}
      />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-baseline gap-x-2">
          <span className="text-sm font-semibold">{server.name}</span>
          <span className="text-[11px] text-muted-foreground">
            {kindInfo(server.kind).label}
            {inUse && (
              <>
                {" "}
                · <Trans>in use</Trans>
              </>
            )}
          </span>
        </div>
        <p className="mt-1 font-display text-xs text-muted-foreground tabular-nums">
          {!server.enabled ? (
            t`Disabled`
          ) : fresh ? (
            <>
              {formatKbps(last.bitrate)} <Trans>kbps</Trans> ·{" "}
              {last.rtt != null ? Math.round(last.rtt) : "—"} <Trans>ms</Trans>
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
        className="max-w-20 shrink-0"
      />
    </div>
  );
}
