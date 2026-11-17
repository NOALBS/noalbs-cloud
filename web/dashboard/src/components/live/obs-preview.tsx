import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Eye, Settings2 } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
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
  type PreviewSettings,
  previewOn,
  qualityOptions,
  usePreviewSettings,
  WIDTH_OPTIONS,
  watchingLine,
} from "@/lib/obs-preview";
import { cn } from "@/lib/utils";
import { PaneTitleRow } from "./pane-title-row";

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
export function useObsFrames(
  workspaceId: string,
  switcherId: string,
  active: boolean,
  fps: number,
  width: number,
  quality: number,
  /** A scene to show instead of the one on air. */
  scene?: string,
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
              params: { path, query: { width, quality, ...(scene && { scene }) } },
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
                ? t`OBS isn’t connected.`
                : response.status === 403
                  ? offAir()
                  : t`OBS didn’t send a picture. Trying again…`,
            );
            wait = Math.max(wait, 3000);
          }
        } catch {
          if (signal.aborted) return;
          setProblem(t`Couldn’t reach NOALBS. Trying again…`);
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
  }, [active, fps, width, quality, workspaceId, switcherId, scene]);

  return { frame, problem };
}

const offAir = () => t`Off air, the preview is hidden. It shows again when the stream goes live.`;

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
    <section aria-label={t`OBS preview`} className="flex flex-col gap-2.5">
      <PaneTitleRow>
        <h2 className="text-[15px] font-semibold tracking-tight">
          <Trans>Preview</Trans>
        </h2>
        {!obsConnected && (
          <span className="ml-2 text-xs text-low">
            <Trans>OBS disconnected · last known state</Trans>
          </span>
        )}
        {watching && (
          <span
            className="ml-auto flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground"
            title={watching}
          >
            <Eye className="size-3.5 shrink-0" aria-hidden />
            <span className="truncate">{watching}</span>
          </span>
        )}
      </PaneTitleRow>

      {/* One box: the picture, a footer bar with the controls, and the settings. */}
      <div className="flex flex-col">
        {on ? (
          <div
            className={cn(
              "live-preview-frame relative -mx-4 flex aspect-video items-center justify-center overflow-hidden border-y bg-black md:-mx-5",
              !obsConnected && "opacity-50",
            )}
          >
            {frame && obsConnected && !hidden && (
              <img
                src={frame.url}
                alt={t`What OBS is showing`}
                className="absolute inset-0 size-full object-contain"
              />
            )}
            {obsConnected && hidden ? (
              <div className="absolute flex max-w-[85%] flex-col items-center gap-1 rounded-lg bg-black/70 px-3 py-2 text-center text-sm text-white">
                <p>{offAir()}</p>
                {privacyLink && (
                  <Link
                    to="/$workspace/s/$switcher/obs"
                    params={privacyLink}
                    hash="preview"
                    className="text-xs underline underline-offset-2"
                  >
                    <Trans>Choose who sees it off air</Trans>
                  </Link>
                )}
              </div>
            ) : (
              (!obsConnected || problem || !frame) && (
                <p className="absolute rounded-lg bg-black/70 px-3 py-1.5 text-sm text-white">
                  {!obsConnected ? t`OBS isn’t connected.` : (problem ?? t`Loading…`)}
                </p>
              )
            )}
          </div>
        ) : (
          <p className="px-4 py-6 text-sm text-muted-foreground">
            <Trans>The preview is off. Turn it on to see what OBS is showing.</Trans>
          </p>
        )}

        <PreviewControls settings={settings} update={update} on={on}>
          {on &&
            frame &&
            !hidden &&
            t`${formatBytes(frame.bytes)} per frame · about ${bandwidthKbps(frame.bytes, settings.fps).toLocaleString(currentLocale())} kbps`}
        </PreviewControls>
      </div>
    </section>
  );
}

export function PreviewControls({
  settings,
  update,
  on,
  children,
  renderToolbar,
}: {
  settings: PreviewSettings;
  update: (patch: Partial<PreviewSettings>) => void;
  on: boolean;
  children?: ReactNode;
  renderToolbar?: (controls: ReactNode) => ReactNode;
}) {
  const [showSettings, setShowSettings] = useState(false);
  const controls = (
    <span className="flex shrink-0 items-center gap-2">
      {on && (
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={t`Preview settings`}
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
        aria-label={t`Show the OBS preview`}
      />
    </span>
  );
  return (
    <>
      {renderToolbar ? (
        renderToolbar(controls)
      ) : (
        <div className="flex min-h-9 items-center justify-between gap-2 py-2">
          <p className="text-xs text-muted-foreground">{children}</p>
          {controls}
        </div>
      )}

      {on && showSettings && (
        <div className="mt-2 grid grid-cols-3 gap-2 border-t border-divider p-3">
          <PreviewSelect
            label={t`Rate`}
            value={settings.fps}
            options={FPS_OPTIONS.map((fps) => ({ value: fps, label: fpsLabel(fps) }))}
            onChange={(fps) => update({ fps })}
          />
          <PreviewSelect
            label={t`Size`}
            value={settings.width}
            options={WIDTH_OPTIONS.map((width) => ({ value: width, label: `${width} px` }))}
            onChange={(width) => update({ width })}
          />
          <PreviewSelect
            label={t`Quality`}
            value={settings.quality}
            options={qualityOptions().map((option) => ({ ...option }))}
            onChange={(quality) => update({ quality })}
          />
        </div>
      )}
    </>
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
