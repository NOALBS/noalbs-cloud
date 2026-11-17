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
        "@container mb-2 min-w-0 rounded-xl border bg-muted px-3.5 py-3 last:mb-0",
        inUse && "border-primary/45",
        !server.enabled && "opacity-60",
      )}
    >
      <div className="flex min-w-0 items-start gap-2">
        <span
          title={t`Priority ${rank ?? server.priority}`}
          className="shrink-0 rounded-md border bg-card px-1.5 font-mono text-[10px] leading-5 font-semibold text-muted-foreground tabular"
        >
          P{rank ?? server.priority}
        </span>
        <span
          className="line-clamp-2 min-w-0 text-sm leading-5 font-semibold [overflow-wrap:anywhere]"
          title={`${server.name} · ${kindInfo(server.kind).label}`}
        >
          {server.name}
        </span>
        <span className="flex-1" />
        {inUse && (
          <span className="shrink-0 rounded-full bg-primary/15 px-2 text-[10px] leading-5 font-semibold tracking-wide text-primary uppercase">
            <Trans>In use</Trans>
          </span>
        )}
        <span className="hidden shrink-0 text-[11px] leading-5 text-subtle-foreground @[18rem]:inline">
          {kindInfo(server.kind).label}
        </span>
      </div>
      <div className="mt-2 flex min-w-0 items-end gap-3">
        {!server.enabled ? (
          <span className="text-sm text-muted-foreground">{t`Disabled`}</span>
        ) : fresh ? (
          <>
            <span className="flex shrink-0 items-baseline gap-1">
              <span className="font-mono text-2xl leading-none font-medium tracking-tight tabular">
                {formatKbps(last.bitrate)}
              </span>
              <span className="text-[11px] text-subtle-foreground">
                <Trans>kbps</Trans>
              </span>
            </span>
            <span className="flex shrink-0 items-baseline gap-1">
              <span className="font-mono text-base leading-none font-medium tabular">
                {last.rtt != null ? Math.round(last.rtt) : "—"}
              </span>
              <span className="text-[11px] text-subtle-foreground">
                <Trans>ms RTT</Trans>
              </span>
            </span>
          </>
        ) : (
          <span className="text-sm font-medium text-offline">{t`No feed`}</span>
        )}
        <BitrateChart
          points={points}
          now={now}
          low={null}
          offline={null}
          thresholds={false}
          tone={look.text}
          height={28}
          className="min-w-10 flex-1 rounded"
        />
      </div>
    </div>
  );
}
