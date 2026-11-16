import { Keyboard, Maximize2, Minimize2 } from "lucide-react";
import type { SwitcherStatus } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { formatUptime } from "@/lib/live-format";
import { cn } from "@/lib/utils";
import { streamState } from "./live-status";
import { type ObsControls, RecordButton, StreamButton } from "./quick-actions";
import type { useOperator } from "./use-operator";

/** The on-air timer beside the page title. */
export function LiveTimer({
  status,
  now,
}: {
  status: SwitcherStatus | null | undefined;
  now: number;
}) {
  const state = streamState(status, now);
  const live = state === "live";
  const unknown = state === "unknown";
  return (
    <span
      className={cn(
        "inline-flex h-8 items-center gap-2 rounded-full px-3 font-mono text-[13px] font-semibold tabular",
        unknown
          ? "bg-low/12 text-low"
          : live
            ? "bg-offline/15 text-offline"
            : "bg-muted text-muted-foreground",
      )}
    >
      <span
        className={cn(
          "size-1.5 rounded-full",
          unknown ? "bg-low" : live ? "bg-offline" : "bg-idle",
        )}
      />
      {unknown
        ? "OBS disconnected"
        : live
          ? status?.stream_started_at
            ? formatUptime(status.stream_started_at, now)
            : "On air"
          : "Off air"}
    </span>
  );
}

/** Desktop header controls: mode, immersive, record and the stream button. */
export function LiveHeaderControls({
  operator,
  status,
  controls,
  disabled,
}: {
  operator: ReturnType<typeof useOperator>;
  status: SwitcherStatus | null | undefined;
  controls: ObsControls;
  disabled: boolean;
}) {
  if (!operator.desktop) return null;
  return (
    <>
      {/* biome-ignore lint/a11y/useSemanticElements: a two-button mode switch */}
      <div
        role="group"
        aria-label="Page mode"
        className="flex gap-0.5 rounded-[10px] border bg-card p-[3px]"
      >
        {(["Streamer", "Operator"] as const).map((name) => {
          const on = operator.enabled === (name === "Operator");
          return (
            <button
              key={name}
              type="button"
              aria-pressed={on}
              onClick={() => operator.setMode(name === "Operator")}
              className={cn(
                "h-7 rounded-[7px] px-3 text-[13px] font-medium text-muted-foreground",
                on && "bg-surface-2 text-foreground",
              )}
            >
              {name}
            </button>
          );
        })}
      </div>
      {operator.enabled && (
        <Button
          variant="outline"
          size="icon-sm"
          aria-label="Keyboard shortcuts"
          title="Keyboard shortcuts (?)"
          onClick={() => operator.setHelp(true)}
        >
          <Keyboard />
        </Button>
      )}
      <Button
        variant="outline"
        size="sm"
        onClick={operator.toggleImmersive}
        title="Hide the sidebar (F)"
      >
        {operator.immersive ? <Minimize2 /> : <Maximize2 />}
        {operator.immersive ? "Exit immersive" : "Immersive"}
      </Button>
      <RecordButton controls={controls} status={status} disabled={disabled} variant="compact" />
      <StreamButton controls={controls} status={status} disabled={disabled} variant="compact" />
      <Dialog open={operator.help} onOpenChange={operator.setHelp}>
        <DialogContent>
          <DialogTitle>Operator shortcuts</DialogTitle>
          <DialogDescription>
            Active in operator mode on desktop, outside fields, dialogs and Edit layout.
          </DialogDescription>
          <dl className="grid grid-cols-[1fr_auto] gap-3 text-sm">
            {[
              ["Cue a scene (switch with Studio mode off)", "1–9"],
              ["Cut directly", "Shift+1–9"],
              ["Take", "Space"],
              ["Cut", "Enter"],
              ["Shortcuts", "?"],
              ["Immersive", "F"],
              ["Exit immersive", "Esc"],
            ].map(([label, key]) => (
              <div key={key} className="contents">
                <dt>{label}</dt>
                <dd className="font-mono text-muted-foreground">{key}</dd>
              </div>
            ))}
          </dl>
          <p className="text-sm text-muted-foreground">
            No key starts or stops streaming or recording. Stopping takes a one-second hold.
          </p>
        </DialogContent>
      </Dialog>
    </>
  );
}
