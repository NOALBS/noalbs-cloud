import { Switch } from "@/components/ui/switch";
import { previewOn, usePreviewSettings } from "@/lib/obs-preview";
import { cn } from "@/lib/utils";
import { useObsFrames } from "./obs-preview";
import { transitions } from "./operator-state";
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
  const { state, setState, studio, preview, program, by, take, pending } = operator;
  const ready =
    studio && Boolean(preview) && preview?.uuid !== program?.uuid && !pending && !operator.disabled;
  const screen =
    "relative flex min-h-0 flex-1 items-center justify-center overflow-hidden rounded-xl bg-black text-center text-sm text-white/60";
  return (
    <section aria-label="Preview and Program" className="flex min-h-0 flex-1 flex-col">
      <div className="flex min-h-0 flex-1 gap-5">
        {studio && (
          <div className={cn(screen, !connected && "opacity-50")}>
            <Label
              tone={connected ? "bg-live" : "bg-low"}
              tag={connected ? "PREVIEW" : "PREVIEW · LAST KNOWN"}
              name={preview?.name}
            />
            <span className="px-6">
              <strong className="block text-lg font-semibold text-white">
                {preview?.name ?? "Cue a scene below"}
              </strong>
              OBS sends no picture for the cued scene yet
            </span>
          </div>
        )}
        <div className={cn(screen, !connected && "opacity-50")}>
          <Label
            tone={!connected ? "bg-low" : streaming ? "bg-offline" : "bg-idle"}
            tag={!connected ? "PROGRAM · LAST KNOWN" : streaming ? "ON AIR" : "PROGRAM · ACTIVE"}
            name={program?.name}
          />
          {allowed && on && frame ? (
            <img src={frame.url} alt="OBS Program output" className="size-full object-contain" />
          ) : (
            <span className="px-6">
              {!connected
                ? "OBS disconnected · stream state unknown"
                : !allowed
                  ? "Off air: the picture is private"
                  : !on
                    ? "The preview is turned off"
                    : (problem ?? "Waiting for the first picture…")}
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
          Take <Kbd dark>Space</Kbd>
        </button>
        <button
          type="button"
          disabled={!ready}
          onClick={take}
          className="inline-flex h-10 items-center gap-3 rounded-xl border bg-muted px-4 font-semibold disabled:opacity-40"
        >
          Cut <Kbd>Enter</Kbd>
        </button>
        {/* biome-ignore lint/a11y/useSemanticElements: a row of toggle buttons */}
        <div
          role="group"
          aria-label="Transition"
          className="ml-1 flex gap-1 rounded-[9px] border bg-muted p-[3px]"
        >
          {transitions.map((name) => (
            <button
              key={name}
              type="button"
              aria-pressed={state.transition === name}
              onClick={() => setState((previous) => ({ ...previous, transition: name }))}
              className={cn(
                "h-6 rounded-md px-2.5 text-xs font-medium text-muted-foreground",
                state.transition === name && "bg-card text-foreground",
              )}
            >
              {name}
            </button>
          ))}
        </div>
        <label className="flex items-center gap-1.5 font-mono text-xs text-muted-foreground">
          <input
            aria-label="Transition duration"
            type="number"
            min={0}
            max={10000}
            step={50}
            value={state.duration}
            onChange={(event) =>
              setState((previous) => ({
                ...previous,
                duration: Math.max(0, Math.min(10000, Number(event.target.value))),
              }))
            }
            className="h-7 w-16 [appearance:textfield] rounded-md border bg-muted px-2 text-right text-foreground [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
          />
          ms
        </label>
        <span className="flex-1" />
        {by && <span className="truncate text-xs text-muted-foreground">Switched by {by}</span>}
        <label className="flex items-center gap-2 text-sm font-medium" htmlFor="live-studio-mode">
          <Switch
            id="live-studio-mode"
            aria-label="Studio mode"
            checked={state.studio}
            onCheckedChange={(value) => setState((previous) => ({ ...previous, studio: value }))}
          />
          Studio mode
        </label>
      </div>
    </section>
  );
}
