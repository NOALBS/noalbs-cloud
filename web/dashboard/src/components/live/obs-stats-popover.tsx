import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { type ReactElement, type ReactNode, useEffect, useRef, useState } from "react";
import { ApiRequestError } from "@/api/client";
import { obsStatsQuery } from "@/api/queries";
import type { ObsDropped, ObsStats } from "@/api/types";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Skeleton } from "@/components/ui/skeleton";
import { errorMessage } from "@/lib/errors";
import { formatKbps } from "@/lib/live-format";
import {
  bitrateBetween,
  droppedTone,
  formatDropped,
  formatMegabytes,
  OBS_STATS_INTERVAL,
} from "@/lib/obs-stats";
import { cn } from "@/lib/utils";

type Path = { workspace_id: string; switcher_id: string };

/**
 * OBS's stats (like its Stats dock and NOALBS's `!serverinfo`) in a popover
 * opened by `children`, the OBS status chip. Asks OBS only while open.
 */
export function ObsStatsPopover({
  path,
  connected,
  connectDevice,
  children,
}: {
  path: Path;
  connected: boolean;
  connectDevice?: string;
  children: ReactElement;
}) {
  return (
    <Popover>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent
        align="end"
        collisionPadding={16}
        className="w-[min(20rem,calc(100vw-2rem))] p-0"
      >
        <div className="flex items-center justify-between gap-3 border-b px-4 py-3">
          <span className="flex items-center gap-2 text-sm font-semibold">
            <span className={cn("size-1.5 rounded-full", connected ? "bg-live" : "bg-offline")} />
            <Trans>OBS</Trans>
          </span>
          <span className="text-xs text-muted-foreground">
            {connected ? t`Connected` : t`Not connected`}
          </span>
        </div>
        {connected ? (
          <ObsStatsBody path={path} />
        ) : (
          <p className="p-4 text-sm text-muted-foreground">
            {connectDevice ? (
              t`OBS isn’t connected to Connect on ${connectDevice}.`
            ) : (
              <Trans>
                OBS isn’t connected. Open OBS with the NOALBS browser source active to see its
                stats.
              </Trans>
            )}
          </p>
        )}
      </PopoverContent>
    </Popover>
  );
}

/** Polls while mounted (the popover is open); two readings give the bitrate. */
function ObsStatsBody({ path }: { path: Path }) {
  const query = useQuery({
    ...obsStatsQuery(path.workspace_id, path.switcher_id),
    refetchInterval: OBS_STATS_INTERVAL,
  });
  const previous = useRef<ObsStats | undefined>(undefined);
  const [bitrate, setBitrate] = useState<number | null>(null);

  useEffect(() => {
    if (!query.data) return;
    const before = previous.current;
    previous.current = query.data;
    if (before) setBitrate(bitrateBetween(before, query.data));
  }, [query.data]);

  const stats = query.data;
  if (!stats) {
    if (query.error) {
      return (
        <p className="p-4 text-sm text-muted-foreground">
          {query.error instanceof ApiRequestError
            ? errorMessage(query.error)
            : t`Couldn’t read OBS’s stats.`}
        </p>
      );
    }
    return (
      <div className="flex flex-col gap-3 p-4">
        <Skeleton className="h-8 w-32" />
        <Skeleton className="h-16" />
        <Skeleton className="h-12" />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4 p-4">
      <div className="flex items-end justify-between gap-3">
        <Stat label={t`Stream bitrate`}>
          {stats.streaming ? (
            bitrate != null ? (
              <>
                <span className="text-2xl">{formatKbps(bitrate)}</span>
                <span className="text-sm font-normal text-muted-foreground">
                  {" "}
                  <Trans>kbps</Trans>
                </span>
              </>
            ) : (
              <span className="text-sm font-normal text-muted-foreground">
                <Trans>Measuring…</Trans>
              </span>
            )
          ) : (
            <span className="text-sm font-normal text-muted-foreground">
              <Trans>Not streaming</Trans>
            </span>
          )}
        </Stat>
        {stats.reconnecting && (
          <span className="rounded-full bg-offline/15 px-2.5 py-1 text-xs font-semibold text-offline">
            <Trans>Reconnecting</Trans>
          </span>
        )}
      </div>

      <div className="flex flex-col gap-1.5">
        <div className="flex items-baseline justify-between gap-2">
          <span className="text-xs tracking-wider text-muted-foreground uppercase">
            <Trans>Dropped frames</Trans>
          </span>
          <span className="text-xs text-muted-foreground">
            {stats.this_stream ? t`This stream` : t`Since OBS started`}
          </span>
        </div>
        {stats.streaming && <DroppedRow label={t`Network`} dropped={stats.network} />}
        <DroppedRow label={t`Rendering lag`} dropped={stats.rendering} />
        <DroppedRow label={t`Encoding lag`} dropped={stats.encoding} />
      </div>

      <div className="grid grid-cols-2 gap-x-4 gap-y-3 border-t border-divider pt-4">
        <Stat label="FPS" detail={`${stats.frame_render_ms.toFixed(1)} ms/frame`}>
          {stats.fps.toFixed(2)}
        </Stat>
        <Stat label="CPU">{stats.cpu_percent.toFixed(1)}%</Stat>
        <Stat label={t`Memory`}>{formatMegabytes(stats.memory_mb)}</Stat>
        <Stat label={t`Free disk`}>{formatMegabytes(stats.disk_free_mb)}</Stat>
      </div>
    </div>
  );
}

const TONE = { ok: "", warn: "text-low", bad: "text-offline" } as const;

function DroppedRow({ label, dropped }: { label: string; dropped: ObsDropped }) {
  return (
    <div className="flex items-center justify-between gap-3 text-sm">
      <span className="text-muted-foreground">{label}</span>
      <span className={cn("font-mono tabular", TONE[droppedTone(dropped)])}>
        {formatDropped(dropped)}
      </span>
    </div>
  );
}

function Stat({
  label,
  detail,
  children,
}: {
  label: string;
  detail?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-xs tracking-wider text-muted-foreground uppercase">{label}</span>
      <span className="font-mono leading-none font-semibold tabular">{children}</span>
      {detail && <span className="text-xs text-muted-foreground">{detail}</span>}
    </div>
  );
}
