import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import type { SwitcherStatus, Triggers } from "@/api/types";
import { Switch } from "@/components/ui/switch";
import { formatKbps } from "@/lib/live-format";
import type { Point } from "@/live/reducer";
import { BitrateChart } from "./bitrate-chart";
import { lookFor } from "./feed-state";
import { hasFeed, streamState } from "./live-status";
import { Pane } from "./pane";
import type { ObsControls } from "./quick-actions";

/** The feed in use: bitrate and RTT now, the last two minutes, and the auto switch toggle. */
export function BitratePane({
  controls,
  status,
  enabled,
  onlyWhileStreaming,
  canControl,
  triggers,
  server,
  sample,
  points,
  now,
}: {
  controls: ObsControls;
  status: SwitcherStatus | null | undefined;
  /** The saved auto switch setting, used until the live value arrives. */
  enabled: boolean;
  onlyWhileStreaming: boolean;
  canControl: boolean;
  triggers: Triggers;
  server: string | undefined;
  sample: Point | undefined;
  points: Point[];
  now: number;
}) {
  const on = status ? status.auto_switch : enabled;
  const waiting = on && onlyWhileStreaming && status?.obs_connected && !status.streaming;
  const available =
    streamState(status, now) !== "unknown" && hasFeed(sample, now) && status?.state !== "offline";
  const current = available ? sample : undefined;
  return (
    <Pane
      title={t`Bitrate`}
      hint={server ?? t`No stream server`}
      label={t`Bitrate`}
      className="flex flex-col"
      tools={
        <label
          htmlFor="live-auto-switch"
          className="flex shrink-0 items-center gap-2 text-xs whitespace-nowrap text-muted-foreground"
        >
          {waiting ? t`Auto switch · waiting for the stream` : t`Auto switch`}
          <Switch
            id="live-auto-switch"
            aria-label={t`Auto switch`}
            checked={on}
            onCheckedChange={(value) => controls.autoSwitch.mutate(value)}
            disabled={!canControl || controls.autoSwitch.isPending}
          />
        </label>
      }
    >
      {!available && (
        <p role="status" className="mb-3 font-semibold text-offline">
          <Trans>
            No feed <span className="text-xs font-normal">· bitrate unavailable</span>
          </Trans>
        </p>
      )}
      <div className="flex items-end gap-6">
        <div>
          <span className="font-mono text-[44px] leading-none font-medium tracking-tighter tabular">
            {current ? formatKbps(current.bitrate) : "—"}
          </span>
          <span className="ml-1.5 text-[13px] text-muted-foreground">
            <Trans>kbps</Trans>
          </span>
        </div>
        <div className="pb-1">
          <span className="font-mono text-xl font-medium tabular">
            {current?.rtt != null ? Math.round(current.rtt) : "—"}
          </span>
          <span className="ml-1.5 text-[13px] text-muted-foreground">
            <Trans>ms RTT</Trans>
          </span>
        </div>
      </div>
      <div className="relative -mx-4 mt-3 min-h-24 flex-1 [--live-chart-height:100%] md:-mx-5">
        <BitrateChart
          points={points}
          now={now}
          low={triggers.low}
          offline={triggers.offline}
          tone={available ? lookFor(status).text : "text-offline"}
          className="absolute inset-0"
        />
      </div>
      <div className="mt-2 flex flex-wrap gap-x-4 text-xs text-muted-foreground">
        {triggers.low != null && (
          <span className="flex items-center gap-1.5">
            <span className="w-3.5 border-t border-dashed border-low" />
            {t`Low ${formatKbps(triggers.low)}`}
          </span>
        )}
        {triggers.offline != null && (
          <span className="flex items-center gap-1.5">
            <span className="w-3.5 border-t border-dashed border-offline" />
            {t`BRB ${formatKbps(triggers.offline)}`}
          </span>
        )}
        <span className="ml-auto">
          <Trans>Last 2 min</Trans>
        </span>
      </div>
    </Pane>
  );
}
