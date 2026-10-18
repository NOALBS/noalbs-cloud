import type { StreamServer } from "@/api/types";
import { Badge } from "@/components/ui/badge";
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
  const last = points[points.length - 1];
  const fresh = last && now - last.at < 5000;
  const look = lookForState(fresh ? last.state : "offline");

  return (
    <div
      className={cn(
        "flex flex-col gap-2 rounded-2xl border bg-card p-3.5",
        inUse && "border-primary-soft-border",
        !server.enabled && "opacity-60",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="flex min-w-0 items-center gap-2">
          <span className="rounded-md border px-1.5 font-mono text-[11px] text-muted-foreground">
            P{server.priority}
          </span>
          <span className="truncate text-sm font-semibold">{server.name}</span>
        </span>
        <Badge className="bg-primary-soft text-[10px] tracking-wider text-primary-soft-foreground uppercase">
          {kindInfo(server.kind).label}
        </Badge>
      </div>
      <div className="flex items-center gap-3">
        <span className="w-28 shrink-0 font-mono text-xl font-semibold tabular">
          {fresh ? formatKbps(last.bitrate) : "—"}
          <span className="text-xs font-normal text-muted-foreground"> kbps</span>
        </span>
        <BitrateChart
          points={points}
          now={now}
          low={null}
          offline={null}
          thresholds={false}
          tone={look.text}
          height={34}
          className="flex-1"
        />
      </div>
      <span className="flex items-center gap-2 font-mono text-xs text-muted-foreground tabular">
        {!server.enabled
          ? "Disabled"
          : fresh
            ? `${last.rtt != null ? `RTT ${Math.round(last.rtt)} ms · ` : ""}${look.label.toLowerCase()}`
            : "No feed"}
        {inUse && (
          <span className="rounded bg-primary px-1.5 font-sans text-[10px] font-bold text-primary-foreground">
            IN USE
          </span>
        )}
      </span>
    </div>
  );
}
