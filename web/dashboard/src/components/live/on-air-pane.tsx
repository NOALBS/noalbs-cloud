import type { SwitcherStatus } from "@/api/types";
import { Pane } from "./pane";
import { FixButton, MarkerButton, type ObsControls } from "./quick-actions";

/** Streamer mode: the scene on air, why it's on, and the two quick actions. */
export function OnAirPane({
  status,
  controls,
  disabled,
  by,
  paused,
}: {
  status: SwitcherStatus | null | undefined;
  controls: ObsControls;
  disabled: boolean;
  /** Who or what switched to this scene, when the activity feed knows. */
  by: string | undefined;
  paused: boolean;
}) {
  return (
    <Pane title={status?.streaming ? "On air" : "In OBS"} label="On air" className="flex flex-col">
      <p className="text-[28px] leading-tight font-semibold tracking-tight [overflow-wrap:anywhere]">
        {status?.current_scene?.name ?? "—"}
      </p>
      <p className="mt-1.5 text-[13px] text-muted-foreground">
        {status === undefined
          ? "Connecting…"
          : !status?.obs_connected
            ? "OBS isn’t connected"
            : paused
              ? "Not switching while paused"
              : by
                ? `Switched by ${by}`
                : status.auto_switch
                  ? "Auto switch is on"
                  : "Auto switch is off"}
      </p>
      <div className="mt-auto grid grid-cols-2 gap-2 pt-4">
        <FixButton controls={controls} disabled={disabled} variant="compact" />
        <MarkerButton controls={controls} status={status} disabled={disabled} variant="compact" />
      </div>
    </Pane>
  );
}
