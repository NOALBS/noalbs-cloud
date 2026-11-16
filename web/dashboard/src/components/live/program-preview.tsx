import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { previewOn, usePreviewSettings } from "@/lib/obs-preview";
import { useObsFrames } from "./obs-preview";
import { transitions } from "./operator-state";
import type { useOperator } from "./use-operator";

export function ProgramPreview({
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
  return (
    <section className="operator-monitors" aria-label="Program / Preview">
      <div className="operator-monitor-header">
        <h2>Program / Preview</h2>
      </div>
      <div className="operator-monitor preview-monitor">
        <div className="operator-picture preview-placeholder">
          <span className="placeholder-label">
            {studio ? "PREVIEW · CUE ONLY" : "STUDIO MODE OFF"}
          </span>
          <strong>{preview?.name ?? "Cue a scene below"}</strong>
          <span>No preview video feed is available</span>
          {preview && program && preview.uuid === program.uuid && (
            <span>Also in Program · source edits affect the on-air scene</span>
          )}
        </div>
        <div className="operator-umd">
          <span className="preview-tally">■ PREVIEW</span>
          <strong>{preview?.name ?? "No scene cued"}</strong>
          <span className="ml-auto text-muted-foreground">local cue</span>
        </div>
      </div>
      <div className="operator-transition">
        <strong>Transition</strong>
        <Button type="button" className="take-button" disabled={!ready} onClick={take}>
          <b>TAKE</b>
          <kbd>Space</kbd>
        </Button>
        <Button
          variant="outline"
          type="button"
          className="cut-button"
          disabled={!ready}
          onClick={take}
        >
          <b>CUT</b>
          <kbd>Enter</kbd>
        </Button>
        <div className="transition-list">
          {transitions.map((name) => (
            <Button
              variant={state.transition === name ? "secondary" : "ghost"}
              size="sm"
              type="button"
              key={name}
              aria-pressed={state.transition === name}
              onClick={() => setState((previous) => ({ ...previous, transition: name }))}
            >
              <span>{name}</span>
              {state.transition === name && (
                <span>{name === "Cut" ? "instant" : `${state.duration} ms`}</span>
              )}
            </Button>
          ))}
        </div>
        <label className="transition-duration" htmlFor="live-transition-duration">
          Rate{" "}
          <Input
            id="live-transition-duration"
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
          />{" "}
          ms
        </label>
        <p>
          Applied in OBS later.
          <br />
          Take and Cut both switch instantly today.
        </p>
      </div>
      <div className="operator-monitor program-monitor">
        <div className="operator-picture program-picture">
          {allowed && on && frame ? (
            <img src={frame.url} alt="OBS Program output" />
          ) : (
            <span>
              {!connected
                ? "OBS disconnected"
                : !allowed
                  ? "Off air · Program image is private"
                  : !on
                    ? "Program image disabled in preview settings"
                    : (problem ?? "Waiting for OBS screenshot…")}
            </span>
          )}
        </div>
        <div className="operator-umd">
          <span className={streaming ? "program-tally" : "text-muted-foreground"}>
            ■ PROGRAM{streaming ? " · ON AIR" : " · OFF AIR"}
          </span>
          <strong>{program?.name ?? "No scene"}</strong>
          {by && <span className="switched-by">{by}</span>}
          <span className="ml-auto text-muted-foreground">{settings.fps} fps · stills</span>
        </div>
        {by && <span className="program-reason">Switched by {by}</span>}
      </div>
    </section>
  );
}
