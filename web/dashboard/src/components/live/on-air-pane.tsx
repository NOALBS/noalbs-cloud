import type { SwitcherStatus } from "@/api/types";
import { cn } from "@/lib/utils";
import { streamState } from "./live-status";
import { Pane } from "./pane";
import { FixButton, MarkerButton, type ObsControls } from "./quick-actions";

/** Streamer mode: the scene on air, why it's on, and the two quick actions. */
export function OnAirPane({
  status,
  controls,
  disabled,
  by,
  paused,
  lastKnownScene,
}: {
  status: SwitcherStatus | null | undefined;
  controls: ObsControls;
  disabled: boolean;
  /** Who or what switched to this scene, when the activity feed knows. */
  by: string | undefined;
  paused: boolean;
  lastKnownScene?: string;
}) {
  const state = streamState(status);
  const unknown = state === "unknown";
  return (
    <Pane
      title={
        unknown ? (
          <span className="text-low">OBS disconnected</span>
        ) : state === "live" ? (
          "On air"
        ) : (
          "In OBS"
        )
      }
      label="On air"
      hint={unknown && lastKnownScene ? "Last known scene" : undefined}
      className="flex flex-col"
    >
      <p
        className={cn(
          "text-[28px] leading-tight font-semibold tracking-tight [overflow-wrap:anywhere]",
          unknown && "opacity-50",
        )}
      >
        {status?.current_scene?.name ?? lastKnownScene ?? "—"}
      </p>
      <p className="mt-1.5 text-[13px] text-muted-foreground">
        {unknown
          ? "Stream state unknown · waiting for OBS"
          : paused
            ? "Not switching while paused"
            : by
              ? `Switched by ${by}`
              : status?.auto_switch
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
