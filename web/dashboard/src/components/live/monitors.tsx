import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useEffect, useState } from "react";
import { Switch } from "@/components/ui/switch";
import { previewOn, usePreviewSettings } from "@/lib/obs-preview";
import { cn } from "@/lib/utils";
import { useObsFrames } from "./obs-preview";
import { clampDuration } from "./operator-state";
import { PaneTitleRow } from "./pane-title-row";
import type { useOperator } from "./use-operator";

function Kbd({ children, dark }: { children: string; dark?: boolean }) {
  return (
    <kbd
      className={cn(
        "rounded-[5px] px-1.5 py-0.5 font-mono text-[10px] font-medium",
        dark ? "bg-black/10 text-black/60" : "bg-surface-2 text-subtle-foreground",
      )}
    >
      {children}
    </kbd>
  );
}

function Label({ tone, tag, name }: { tone: string; tag: string; name: string | undefined }) {
  return (
    <span className="absolute top-3 left-3 z-1 flex h-[26px] items-center gap-2 rounded-lg bg-black/70 px-2.5 text-xs font-semibold text-white backdrop-blur">
      <span className={cn("size-1.5 rounded-full", tone)} />
      <small className={cn("text-[10px] font-bold tracking-wider", tone.replace("bg-", "text-"))}>
        {tag}
      </small>
      {name}
    </span>
  );
}

/** Operator mode: Preview and Program side by side, with Take, Cut and the transition under them. */
export function Monitors({
  operator,
  path,
  connected,
  streaming,
  offAirAllowed,
}: {
  operator: ReturnType<typeof useOperator>;
  path: { workspace_id: string; switcher_id: string };
  connected: boolean;
  streaming: boolean;
  offAirAllowed: boolean;
}) {
  const [settings] = usePreviewSettings();
  const allowed = connected && (streaming || offAirAllowed);
  const on = previewOn(settings, operator.desktop);
  const { frame, problem } = useObsFrames(
    path.workspace_id,
    path.switcher_id,
    allowed && on,
    settings.fps,
    settings.width,
    settings.quality,
  );
  const { state, setState, studio, preview, program, by, take, cut, pending } = operator;
  const { transitions, transition } = operator;
  // The cued scene isn't on air, so its picture follows the off-air privacy
  // setting even while streaming. A couple of frames a second is plenty for it.
  const cued = useObsFrames(
    path.workspace_id,
    path.switcher_id,
    studio && connected && offAirAllowed && on && Boolean(preview),
    Math.min(settings.fps, 2),
    settings.width,
    settings.quality,
    preview?.uuid,
  );
  const [duration, setDuration] = useState(String(state.duration));
  useEffect(() => setDuration(String(state.duration)), [state.duration]);
  const ready =
    studio && Boolean(preview) && preview?.uuid !== program?.uuid && !pending && !operator.disabled;
  const screen =
    "relative flex min-h-0 flex-1 items-center justify-center overflow-hidden rounded-xl bg-black text-center text-sm text-white/60";
  return (
    <section aria-label={t`Preview and Program`} className="flex min-h-0 flex-1 flex-col">
      <PaneTitleRow layoutOnly className="mb-3.5">
        <h2 className="text-[15px] font-semibold tracking-tight">
          <Trans>Preview / Program</Trans>
        </h2>
      </PaneTitleRow>
      <div
        className="live-monitor-screens flex min-h-0 gap-5"
        style={{ aspectRatio: studio ? "32 / 9" : "16 / 9" }}
      >
        {studio && (
          <div className={cn(screen, !connected && "opacity-50")}>
            <Label
              tone={connected ? "bg-live" : "bg-low"}
              tag={connected ? t`PREVIEW` : t`PREVIEW · LAST KNOWN`}
              name={preview?.name}
            />
            {cued.frame && preview ? (
              <img
                src={cued.frame.url}
                alt={t`The cued scene`}
                className="size-full object-contain"
              />
            ) : (
              <span className="px-6">
                <strong className="block text-lg font-semibold text-white">
                  {preview?.name ?? <Trans>Cue a scene below</Trans>}
                </strong>
                {!preview
                  ? null
                  : !connected
                    ? t`OBS disconnected`
                    : !offAirAllowed
                      ? t`Its picture is private until it’s on air`
                      : !on
                        ? t`The preview is turned off`
                        : (cued.problem ?? t`Waiting for the first picture…`)}
              </span>
            )}
          </div>
        )}
        <div className={cn(screen, !connected && "opacity-50")}>
          <Label
            tone={!connected ? "bg-low" : streaming ? "bg-offline" : "bg-idle"}
            tag={!connected ? t`PROGRAM · LAST KNOWN` : streaming ? t`ON AIR` : t`PROGRAM · ACTIVE`}
            name={program?.name}
          />
          {allowed && on && frame ? (
            <img
              src={frame.url}
              alt={t`What OBS is showing`}
              className="size-full object-contain"
            />
          ) : (
            <span className="px-6">
              {!connected
                ? t`OBS disconnected · stream state unknown`
                : !allowed
                  ? t`Off air: the picture is private`
                  : !on
                    ? t`The preview is turned off`
                    : (problem ?? t`Waiting for the first picture…`)}
            </span>
          )}
        </div>
      </div>
      <div className="mt-4 flex flex-wrap items-center gap-2.5 border-t pt-4">
        <button
          type="button"
          disabled={!ready}
          onClick={take}
          className="inline-flex h-10 items-center gap-3 rounded-xl bg-foreground px-4.5 font-semibold text-background disabled:opacity-40"
        >
          <Trans>Take</Trans> <Kbd dark>Space</Kbd>
        </button>
        <button
          type="button"
          disabled={!ready}
          onClick={cut}
          className="inline-flex h-10 items-center gap-3 rounded-xl border bg-muted px-4 font-semibold disabled:opacity-40"
        >
          <Trans>Cut</Trans> <Kbd>Enter</Kbd>
        </button>
        {/* biome-ignore lint/a11y/useSemanticElements: a row of toggle buttons */}
        <div
          role="group"
          aria-label={t`Transition`}
          className="ml-1 flex gap-1 rounded-[9px] border bg-muted p-[3px]"
        >
          {transitions.map(({ name }) => (
            <button
              key={name}
              type="button"
              aria-pressed={transition?.name === name}
              disabled={operator.disabled}
              onClick={() => setState((previous) => ({ ...previous, transition: name }))}
              className={cn(
                "h-6 max-w-40 truncate rounded-md px-2.5 text-xs font-medium text-muted-foreground",
                transition?.name === name && "bg-card text-foreground",
              )}
            >
              {name}
            </button>
          ))}
          {transitions.length === 0 && (
            <span className="flex h-6 items-center px-2.5 text-xs text-subtle-foreground">
              {connected ? <Trans>Loading transitions…</Trans> : <Trans>OBS disconnected</Trans>}
            </span>
          )}
        </div>
        <label
          className={cn(
            "flex items-center gap-1.5 font-mono text-xs text-muted-foreground",
            (!transition || transition.fixed) && "opacity-50",
          )}
          title={transition?.fixed ? t`${transition.name} has a fixed duration` : undefined}
        >
          <input
            aria-label={t`Transition duration`}
            type="number"
            min={50}
            max={20000}
            step={50}
            disabled={!transition || transition.fixed}
            value={duration}
            onChange={(event) => setDuration(event.target.value)}
            onBlur={() => {
              const next = clampDuration(Number(duration));
              setDuration(String(next));
              setState((previous) => ({ ...previous, duration: next }));
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") event.currentTarget.blur();
            }}
            className="h-7 w-16 [appearance:textfield] rounded-md border bg-muted px-2 text-right text-foreground [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
          />
          ms
        </label>
        <span className="flex-1" />
        {by && (
          <span className="truncate text-xs text-muted-foreground">
            <Trans>Switched by {by}</Trans>
          </span>
        )}
        <label className="flex items-center gap-2 text-sm font-medium" htmlFor="live-studio-mode">
          <Switch
            id="live-studio-mode"
            aria-label={t`Studio mode`}
            checked={state.studio}
            onCheckedChange={(value) => setState((previous) => ({ ...previous, studio: value }))}
          />
          <Trans>Studio mode</Trans>
        </label>
      </div>
    </section>
  );
}
