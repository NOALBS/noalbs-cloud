import type { SwitcherStatus, Triggers } from "@/api/types";
import { Switch } from "@/components/ui/switch";
import type { Point } from "@/live/reducer";
import { BitrateChart } from "./bitrate-chart";
import type { ObsControls } from "./quick-actions";

export function AutoSwitchPane({
  controls,
  status,
  enabled,
  onlyWhileStreaming,
  canControl,
  triggers,
  watching,
  points,
  now,
}: {
  controls: ObsControls;
  status: SwitcherStatus | null | undefined;
  enabled: boolean;
  onlyWhileStreaming: boolean;
  canControl: boolean;
  triggers: Triggers;
  watching: string | undefined;
  points: Point[];
  now: number;
}) {
  const on = status?.auto_switch ?? enabled;
  const waiting = on && onlyWhileStreaming && status?.obs_connected && !status.streaming;
  const threshold = (bitrate: number | null | undefined, rtt: number | null | undefined) =>
    [
      bitrate == null ? null : `≤ ${bitrate.toLocaleString()} kbps`,
      rtt == null ? null : `≥ ${rtt.toLocaleString()} ms RTT`,
    ]
      .filter(Boolean)
      .join(" or ") || "Triggers off";
  return (
    <section className="flex flex-col gap-3" aria-label="Auto switch">
      <div className="flex items-center justify-between gap-3">
        <h2 className="font-semibold">Auto switch</h2>
        <Switch
          aria-label="Auto switch"
          checked={on}
          onCheckedChange={(value) => controls.autoSwitch.mutate(value)}
          disabled={!canControl || controls.autoSwitch.isPending}
        />
      </div>
      <p className="text-xs text-muted-foreground">
        {waiting
          ? "Waiting for OBS to stream"
          : on
            ? "Watching the active feed"
            : "Paused · manual scene switching"}
      </p>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
        <dt className="text-muted-foreground">Watching</dt>
        <dd>{watching ?? "No active server"}</dd>
        <dt className="text-muted-foreground">LOW when</dt>
        <dd>{threshold(triggers.low, triggers.rtt)}</dd>
        <dt className="text-muted-foreground">BRB when</dt>
        <dd>{threshold(triggers.offline, triggers.rtt_offline)}</dd>
      </dl>
      <BitrateChart
        points={points}
        now={now}
        low={triggers.low}
        offline={triggers.offline}
        tone="text-primary"
        height={84}
      />
      <span className="text-xs text-muted-foreground">
        Bitrate · LOW / BRB thresholds · last 2 min
      </span>
    </section>
  );
}
