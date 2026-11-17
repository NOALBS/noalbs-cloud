import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { Keyboard, Maximize2, Minimize2 } from "lucide-react";
import { useEffect, useRef } from "react";
import type { SwitcherStatus } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { formatUptime } from "@/lib/live-format";
import { cn } from "@/lib/utils";
import { streamState } from "./live-status";
import type { useOperator } from "./use-operator";

export function LiveHeaderAnchor({ top }: { top: number }) {
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    const anchor = ref.current;
    const root = anchor?.closest<HTMLElement>(".live-root");
    if (!anchor || !root || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        root.dataset.stuck = String(!entry?.isIntersecting);
      },
      { rootMargin: `-${top}px 0px 0px 0px` },
    );
    observer.observe(anchor);
    return () => {
      observer.disconnect();
      delete root.dataset.stuck;
    };
  }, [top]);
  return <span ref={ref} className="live-header-anchor" aria-hidden="true" />;
}

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
        ? t`OBS disconnected`
        : live
          ? status?.stream_started_at
            ? t`ON AIR ${formatUptime(status.stream_started_at, now)}`
            : t`On air`
          : t`Off air`}
    </span>
  );
}

/** The recording timer beside the on-air timer; absent while not recording. */
export function RecordingStatus({
  status,
  now,
}: {
  status: SwitcherStatus | null | undefined;
  now: number;
}) {
  if (streamState(status, now) === "unknown" || !status?.recording) return null;
  const paused = status.recording_paused;
  return (
    <span
      className={cn(
        "inline-flex h-8 items-center gap-2 rounded-full border px-3 font-mono text-[13px] font-semibold tabular",
        paused ? "border-low/40 text-low" : "border-offline/40 text-offline",
      )}
    >
      <span className={cn("size-1.5 rounded-full", paused ? "bg-low" : "bg-offline")} />
      {paused
        ? t`REC paused`
        : status.recording_started_at
          ? t`REC ${formatUptime(status.recording_started_at, now)}`
          : t`REC`}
    </span>
  );
}

/** Header controls, with read-only recording status on phones. */
export function LiveHeaderControls({ operator }: { operator: ReturnType<typeof useOperator> }) {
  const immersive = (
    <Button
      variant="outline"
      size="icon-sm"
      aria-label={operator.immersive ? t`Exit immersive` : t`Immersive`}
      title={operator.immersive ? t`Show the sidebar (F)` : t`Immersive: hide the sidebar (F)`}
      onClick={operator.toggleImmersive}
    >
      {operator.immersive ? <Minimize2 /> : <Maximize2 />}
    </Button>
  );
  if (!operator.desktop) return <span className="ml-auto flex">{immersive}</span>;
  return (
    <>
      {/* biome-ignore lint/a11y/useSemanticElements: a two-button mode switch */}
      <div
        role="group"
        aria-label={t`Page mode`}
        className="flex gap-0.5 rounded-[10px] border bg-card p-[3px]"
      >
        {([false, true] as const).map((mode) => {
          const on = operator.enabled === mode;
          return (
            <button
              key={String(mode)}
              type="button"
              aria-pressed={on}
              onClick={() => operator.setMode(mode)}
              className={cn(
                "h-6.5 rounded-[7px] px-3 text-[13px] font-medium text-muted-foreground",
                on && "bg-surface-2 text-foreground",
              )}
            >
              {mode ? t`Operator` : t`Streamer`}
            </button>
          );
        })}
      </div>
      {operator.enabled && (
        <Button
          variant="outline"
          size="icon-sm"
          aria-label={t`Keyboard shortcuts`}
          title={t`Keyboard shortcuts (?)`}
          onClick={() => operator.setHelp(true)}
        >
          <Keyboard />
        </Button>
      )}
      {immersive}
      <Dialog open={operator.help} onOpenChange={operator.setHelp}>
        <DialogContent>
          <DialogTitle>
            <Trans>Operator shortcuts</Trans>
          </DialogTitle>
          <DialogDescription>
            <Trans>
              Active in operator mode on desktop, outside fields, dialogs and Edit layout.
            </Trans>
          </DialogDescription>
          <dl className="grid grid-cols-[1fr_auto] gap-3 text-sm">
            {[
              [t`Cue a scene (switch with Studio mode off)`, "1–9"],
              [t`Cut directly`, "Shift+1–9"],
              [t`Search scenes`, "/"],
              [t`Take`, t`Space`],
              [t`Cut`, t`Enter`],
              [t`Shortcuts`, "?"],
              [t`Immersive`, "F"],
              [t`Exit immersive`, t`Esc`],
            ].map(([label, key]) => (
              <div key={key} className="contents">
                <dt>{label}</dt>
                <dd className="font-mono text-muted-foreground">{key}</dd>
              </div>
            ))}
          </dl>
          <p className="text-sm text-muted-foreground">
            <Trans>No key starts or stops streaming or recording. Stopping asks to confirm.</Trans>
          </p>
        </DialogContent>
      </Dialog>
    </>
  );
}
