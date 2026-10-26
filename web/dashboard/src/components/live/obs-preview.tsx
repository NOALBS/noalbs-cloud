import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Eye, Settings2 } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "@/api/client";
import { previewWatchersQuery } from "@/api/queries";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { DESKTOP, useMediaQuery } from "@/hooks/use-media-query";
import { currentLocale } from "@/lib/locale";
import {
  bandwidthKbps,
  FPS_OPTIONS,
  formatBytes,
  fpsLabel,
  previewOn,
  QUALITY_OPTIONS,
  usePreviewSettings,
  WIDTH_OPTIONS,
  watchingLine,
} from "@/lib/obs-preview";
import { cn } from "@/lib/utils";

type Path = { workspace_id: string; switcher_id: string };

interface Frame {
  url: string;
  bytes: number;
}

const sleep = (ms: number, signal: AbortSignal) =>
  new Promise<void>((resolve) => {
    const timer = setTimeout(resolve, ms);
    signal.addEventListener("abort", () => {
      clearTimeout(timer);
      resolve();
    });
  });

/**
 * Fetches stills of the scene on air, one after another at `fps` (never more
 * than one request at a time), while `active` and the tab is visible.
 */
function useObsFrames(
  workspaceId: string,
  switcherId: string,
  active: boolean,
  fps: number,
  width: number,
  quality: number,
) {
  const [frame, setFrame] = useState<Frame | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    if (!active) {
      setFrame(null);
      setProblem(null);
      return;
    }
    const controller = new AbortController();
    const { signal } = controller;
    const path = { workspace_id: workspaceId, switcher_id: switcherId };
    let shown: string | null = null;

    const loop = async () => {
      while (!signal.aborted) {
        if (document.hidden) {
          await sleep(1000, signal);
          continue;
        }
        const started = performance.now();
        let wait = 1000 / fps;
        try {
          const { data, response } = await api.GET(
            "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/screenshot",
            {
              params: { path, query: { width, quality } },
              parseAs: "blob",
              signal,
            },
          );
          if (response.ok && data) {
            const url = URL.createObjectURL(data);
            if (shown) URL.revokeObjectURL(shown);
            shown = url;
            setFrame({ url, bytes: data.size });
            setProblem(null);
          } else {
            setProblem(
              response.status === 503
                ? "OBS isn’t connected."
                : response.status === 403
                  ? OFF_AIR
                  : "OBS didn’t send a picture. Trying again…",
            );
            wait = Math.max(wait, 3000);
          }
        } catch {
          if (signal.aborted) return;
          setProblem("Couldn’t reach NOALBS. Trying again…");
          wait = Math.max(wait, 3000);
        }
        await sleep(Math.max(0, wait - (performance.now() - started)), signal);
      }
    };
    void loop();

    return () => {
      controller.abort();
      if (shown) URL.revokeObjectURL(shown);
    };
  }, [active, fps, width, quality, workspaceId, switcherId]);

  return { frame, problem };
}

const OFF_AIR = "Off air, the preview is hidden. It shows again when the stream goes live.";

/**
 * A live still of the OBS output. Off by default on phones, to spare data.
 * Off air it shows only to whom the switcher's setting says (`offAirAllowed`),
 * since OBS can show what the streamer keeps to themselves; `privacyLink` is
 * where the owner changes that. Everyone sees who else is watching.
 */
export function ObsPreview({
  path,
  obsConnected,
  streaming,
  offAirAllowed,
  privacyLink,
}: {
  path: Path;
  obsConnected: boolean;
  streaming: boolean;
  offAirAllowed: boolean;
  /** Route params for the switcher settings, for those who may change who sees it. */
  privacyLink?: { workspace: string; switcher: string };
}) {
  const desktop = useMediaQuery(DESKTOP);
  const [settings, update] = usePreviewSettings();
  const [showSettings, setShowSettings] = useState(false);
  const on = previewOn(settings, desktop);
  const hidden = !streaming && !offAirAllowed;
  const { frame, problem } = useObsFrames(
    path.workspace_id,
    path.switcher_id,
    on && obsConnected && !hidden,
    settings.fps,
    settings.width,
    settings.quality,
  );
  const watchers = useQuery({
    ...previewWatchersQuery(path.workspace_id, path.switcher_id),
    enabled: obsConnected,
  });
  const watching = obsConnected && watchers.data ? watchingLine(watchers.data) : null;

  // Laid out like the other Live sections: heading above, content below, and
  // the controls under it on the right.
  return (
    <section aria-label="OBS preview" className="flex flex-col gap-2.5">
      <div className="flex min-h-8 items-center">
        <h2 className="font-semibold">Preview</h2>
      </div>

      {/* One box: the picture, a footer bar with the controls, and the settings. */}
      <div className="flex flex-col overflow-hidden rounded-2xl border bg-card">
        {on ? (
          <div className="relative flex aspect-video items-center justify-center bg-black">
            {frame && obsConnected && !hidden && (
              <img src={frame.url} alt="What OBS is showing" className="size-full object-contain" />
            )}
            {obsConnected && hidden ? (
              <div className="absolute flex max-w-[85%] flex-col items-center gap-1 rounded-lg bg-black/70 px-3 py-2 text-center text-sm text-white">
                <p>{OFF_AIR}</p>
                {privacyLink && (
                  <Link
                    to="/$workspace/s/$switcher/obs"
                    params={privacyLink}
                    hash="preview"
                    className="text-xs underline underline-offset-2"
                  >
                    Choose who sees it off air
                  </Link>
                )}
              </div>
            ) : (
              (!obsConnected || problem || !frame) && (
                <p className="absolute rounded-lg bg-black/70 px-3 py-1.5 text-sm text-white">
                  {!obsConnected ? "OBS isn’t connected." : (problem ?? "Loading…")}
                </p>
              )
            )}
          </div>
        ) : (
          <p className="px-4 py-6 text-sm text-muted-foreground">
            The preview is off. Turn it on to see what OBS is showing.
          </p>
        )}

        {watching && (
          <p className="flex items-center gap-1.5 border-t border-divider px-4 py-2 text-xs text-muted-foreground">
            <Eye className="size-3.5 shrink-0" aria-hidden />
            {watching}
          </p>
        )}

        <div className="flex min-h-11 items-center justify-between gap-2 border-t border-divider py-1.5 pr-2.5 pl-4">
          <p className="text-xs text-muted-foreground">
            {on &&
              frame &&
              !hidden &&
              `${formatBytes(frame.bytes)} per frame · about ${bandwidthKbps(frame.bytes, settings.fps).toLocaleString(currentLocale())} kbps`}
          </p>
          <span className="flex items-center gap-2">
            {on && (
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Preview settings"
                aria-expanded={showSettings}
                onClick={() => setShowSettings((shown) => !shown)}
                className={cn(showSettings && "bg-surface-2")}
              >
                <Settings2 />
              </Button>
            )}
            <Switch
              checked={on}
              onCheckedChange={(enabled) => update({ enabled })}
              aria-label="Show the OBS preview"
            />
          </span>
        </div>

        {on && showSettings && (
          <div className="grid grid-cols-3 gap-2 border-t border-divider p-3">
            <PreviewSelect
              label="Rate"
              value={settings.fps}
              options={FPS_OPTIONS.map((fps) => ({ value: fps, label: fpsLabel(fps) }))}
              onChange={(fps) => update({ fps })}
            />
            <PreviewSelect
              label="Size"
              value={settings.width}
              options={WIDTH_OPTIONS.map((width) => ({ value: width, label: `${width} px` }))}
              onChange={(width) => update({ width })}
            />
            <PreviewSelect
              label="Quality"
              value={settings.quality}
              options={QUALITY_OPTIONS.map((option) => ({ ...option }))}
              onChange={(quality) => update({ quality })}
            />
          </div>
        )}
      </div>
    </section>
  );
}

function PreviewSelect({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: number;
  options: { value: number; label: string }[];
  onChange: (value: number) => void;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <span className="text-xs text-muted-foreground">{label}</span>
      <Select value={String(value)} onValueChange={(next) => onChange(Number(next))}>
        <SelectTrigger aria-label={label} className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {options.map((option) => (
            <SelectItem key={option.value} value={String(option.value)}>
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}
