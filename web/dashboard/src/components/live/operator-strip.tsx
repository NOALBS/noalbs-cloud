import type { SwitcherStatus } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Switch } from "@/components/ui/switch";
import { formatUptime } from "@/lib/live-format";
import { type ObsControls, StreamButton } from "./quick-actions";
import type { useOperator } from "./use-operator";

export function OperatorHeader({
  operator,
  status,
  controls,
  disabled,
  now,
}: {
  operator: ReturnType<typeof useOperator>;
  status: SwitcherStatus | null | undefined;
  controls: ObsControls;
  disabled: boolean;
  now: number;
}) {
  if (!operator.desktop) return null;
  return (
    <>
      <span className="text-xs tabular-nums text-muted-foreground">
        {status?.streaming
          ? `ON AIR ${status.stream_started_at ? formatUptime(status.stream_started_at, now) : ""}`
          : "Off air"}
        {status?.recording && status.recording_started_at
          ? ` · REC ${formatUptime(status.recording_started_at, now)}`
          : ""}
        {operator.enabled && ` · LOCAL ${new Date(now).toLocaleTimeString([], { hour12: false })}`}
      </span>
      {operator.desktop && (
        <>
          <label className="flex items-center gap-2 text-sm" htmlFor="live-operator-mode">
            <Switch
              id="live-operator-mode"
              aria-label="Operator mode"
              checked={operator.enabled}
              onCheckedChange={operator.setMode}
            />
            Operator mode
          </label>
          {operator.enabled && (
            <>
              <label className="flex items-center gap-2 text-sm" htmlFor="live-studio-mode">
                <Switch
                  id="live-studio-mode"
                  aria-label="Studio mode"
                  checked={operator.state.studio}
                  onCheckedChange={(studio) =>
                    operator.setState((previous) => ({ ...previous, studio }))
                  }
                />
                Studio mode
              </label>
              <Button variant="outline" size="sm" onClick={() => operator.setHelp(true)}>
                Shortcuts ?
              </Button>
            </>
          )}
          <Button variant="outline" size="sm" onClick={operator.toggleImmersive}>
            Immersive
          </Button>
        </>
      )}
      {operator.enabled && (
        <StreamButton controls={controls} status={status} disabled={disabled} variant="compact" />
      )}
      <Dialog open={operator.help} onOpenChange={operator.setHelp}>
        <DialogContent>
          <DialogTitle>Operator shortcuts</DialogTitle>
          <DialogDescription>
            Active only in operator mode on desktop, outside fields, dialogs and Customize.
          </DialogDescription>
          <dl className="grid grid-cols-[1fr_auto] gap-3 text-sm">
            {[
              ["Cue / switch with Studio off", "1–9"],
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
            No hotkey starts or stops streaming or recording. Stop controls require a one-second
            hold.
          </p>
        </DialogContent>
      </Dialog>
    </>
  );
}
