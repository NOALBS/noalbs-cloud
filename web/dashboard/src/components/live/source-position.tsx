import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { RotateCcw } from "lucide-react";
import { type FormEvent, useEffect, useId, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import type { components } from "@/api/schema";
import type { ObsSource } from "@/api/types";
import { PlacementCanvas } from "@/components/live/placement-canvas";
import { MoreSettings, NumberField, Toggle } from "@/components/live/source-fields";
import { Button } from "@/components/ui/button";
import { DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import { Slider } from "@/components/ui/slider";
import { errorMessage } from "@/lib/errors";
import {
  type Box,
  croppedSize,
  inside,
  presetPlacement,
  rounded,
  startingBox,
} from "@/lib/placement";
import { cn } from "@/lib/utils";

type Path = { workspace_id: string; switcher_id: string };
type Fit = components["schemas"]["Fit"];
type Anchor = components["schemas"]["Anchor"];
type Layout = components["schemas"]["SceneLayoutDto"];
type Crop = components["schemas"]["Crop"];

const ANCHORS: Anchor[] = [
  "top_left",
  "top",
  "top_right",
  "left",
  "center",
  "right",
  "bottom_left",
  "bottom",
  "bottom_right",
];

const anchorLabels = (): Record<Anchor, string> => ({
  top_left: t`Top left`,
  top: t`Top`,
  top_right: t`Top right`,
  left: t`Left`,
  center: t`Center`,
  right: t`Right`,
  bottom_left: t`Bottom left`,
  bottom: t`Bottom`,
  bottom_right: t`Bottom right`,
});

const fitLabels = (): Record<Fit, [string, string]> => ({
  fit: [t`Fit`, t`As large as fits, keeping its shape.`],
  fill: [t`Fill`, t`Covers the box, keeping its shape; the edges are cut off.`],
  stretch: [t`Stretch`, t`Exactly the box, out of shape if need be.`],
  original: [t`Original`, t`Its own size.`],
});

/**
 * One still of the scene on air, for drawing under the box: the same
 * picture (and the same rules on who sees it off air) as the Live page's
 * preview. `version` takes a new one.
 */
function usePreviewStill(path: Path, enabled: boolean, version: number) {
  const { workspace_id, switcher_id } = path;
  const [still, setStill] = useState<{ url: string | null; problem: string | null }>({
    url: null,
    problem: null,
  });
  // biome-ignore lint/correctness/useExhaustiveDependencies: `version` asks for a new picture
  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    let url: string | null = null;
    void (async () => {
      try {
        const { data, response } = await api.GET(
          "/workspaces/{workspace_id}/switchers/{switcher_id}/obs/screenshot",
          {
            params: { path: { workspace_id, switcher_id }, query: { width: 640, quality: 60 } },
            parseAs: "blob",
            signal: controller.signal,
          },
        );
        if (response.ok && data) {
          url = URL.createObjectURL(data);
          setStill({ url, problem: null });
        } else {
          setStill({
            url: null,
            problem:
              response.status === 403
                ? t`Off air, the preview is hidden: the box is drawn without it.`
                : t`OBS didn’t send a picture: the box is drawn without it.`,
          });
        }
      } catch {
        // Closed, or offline: the box works without it.
      }
    })();
    return () => {
      controller.abort();
      if (url) URL.revokeObjectURL(url);
    };
  }, [enabled, version, workspace_id, switcher_id]);
  return enabled ? still : { url: null, problem: null };
}

/**
 * Where a source sits on screen: dragged in a drawing of the screen (with the
 * preview behind it when that lines up), a preset, or an exact box in canvas
 * pixels. Nothing changes in OBS until Save; the dialog stays open after, so
 * it can be adjusted again.
 */
export function SourcePosition({ path, source }: { path: Path; source: ObsSource }) {
  const layoutKey = [
    ...queryKeys.obsSources(path.workspace_id, path.switcher_id),
    "layout",
    source.parent,
    source.parent_is_group,
  ];
  const layout = useQuery({
    queryKey: layoutKey,
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scene-layout", {
          params: { path, query: { scene: source.parent, group: source.parent_is_group } },
        }),
      ),
    retry: false,
    gcTime: 0,
    staleTime: Infinity,
  });
  const item = layout.data?.items.find((entry) => entry.item_id === source.item_id);

  return (
    <>
      <DialogHeader>
        <DialogTitle>
          <Trans>Position {source.name}</Trans>
        </DialogTitle>
        <DialogDescription>
          <Trans>Nothing changes in OBS until you save.</Trans>
        </DialogDescription>
      </DialogHeader>
      {layout.isPending ? (
        <Skeleton className="aspect-video" />
      ) : layout.isError ? (
        <p className="text-sm text-destructive">{errorMessage(layout.error)}</p>
      ) : !item ? (
        <p className="text-sm text-muted-foreground">
          <Trans>It isn’t in {source.parent} anymore.</Trans>
        </p>
      ) : (
        <PositionForm
          path={path}
          source={source}
          layout={layout.data}
          item={item}
          onSaved={() => layout.refetch()}
        />
      )}
    </>
  );
}

function PositionForm({
  path,
  source,
  layout,
  item,
  onSaved,
}: {
  path: Path;
  source: ObsSource;
  layout: Layout;
  item: Layout["items"][number];
  onSaved: () => Promise<unknown>;
}) {
  const ids = useId();
  const queryClient = useQueryClient();
  const canvas = { width: layout.canvas_width, height: layout.canvas_height };
  const start = startingBox(item, canvas);
  const [mode, setMode] = useState<"drag" | "preset" | "exact">("drag");
  const [box, setBox] = useState<Box>(start.box);
  const [crop, setCrop] = useState<Crop>(item.crop);
  const [keepShape, setKeepShape] = useState(true);
  const [fit, setFit] = useState<Fit>("fit");
  const [anchor, setAnchor] = useState<Anchor>("center");
  const [size, setSize] = useState(100);
  const [margin, setMargin] = useState(3);
  const [stillVersion, setStillVersion] = useState(0);
  // Only a source right on the scene on air lines up with the preview.
  const linesUp = source.depth === 0 && !source.parent_is_group;
  const still = usePreviewStill(path, linesUp, stillVersion);
  const labels = anchorLabels();
  const fits = fitLabels();

  const save = useMutation({
    mutationFn: () =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/source/placement", {
          params: { path },
          body: {
            parent: source.parent,
            parent_is_group: source.parent_is_group,
            item_id: source.item_id,
            placement:
              mode === "preset"
                ? { mode, fit, anchor, size_percent: size, margin_percent: margin }
                : { mode: "exact", ...rounded(box), crop },
          },
        }),
      ),
    onSuccess: async () => {
      toast.success(t`Moved ${source.name}`);
      void queryClient.invalidateQueries({
        queryKey: queryKeys.obsSources(path.workspace_id, path.switcher_id),
        exact: false,
        predicate: (query) => !query.queryKey.includes("layout"),
      });
      // Where it is now (a preset's box comes from OBS), and a new picture.
      await onSaved();
      setStillVersion((version) => version + 1);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  // After saving, start again from where OBS put it.
  // biome-ignore lint/correctness/useExhaustiveDependencies: only when the saved layout comes back
  useEffect(() => {
    setBox(startingBox(item, canvas).box);
    setCrop(item.crop);
  }, [layout]);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    save.mutate();
  };

  const toggle = (pressed: boolean) =>
    cn(
      "h-8 rounded-md text-sm font-medium outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
      pressed
        ? "bg-primary-soft font-semibold text-primary-soft-foreground"
        : "text-muted-foreground hover:text-foreground",
    );

  // The source's own size once cropped (by the crop being edited, outside presets).
  const ownSize = croppedSize(
    { width: item.source_width, height: item.source_height },
    mode === "preset" ? item.crop : crop,
  );
  // What's drawn: where the preset would put it, or the box (it fits inside, centered).
  const shown: { box: Box; content: Box | null } | null =
    mode === "preset"
      ? presetPlacement({ fit, anchor, sizePercent: size, marginPercent: margin }, canvas, ownSize)
      : { box, content: inside(box, ownSize, "fit") };

  const others = layout.items
    .filter((entry) => entry.item_id !== item.item_id && entry.width >= 1 && entry.height >= 1)
    .map((entry) => ({
      key: String(entry.item_id),
      name: entry.name,
      enabled: entry.enabled,
      box: entry,
    }))
    // Bottom first, so the top ones are drawn last.
    .reverse();

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-col gap-4">
      <div className="-mx-1 flex min-h-0 flex-col gap-4 overflow-y-auto px-1">
        <div className="grid grid-cols-3 rounded-lg border p-0.5">
          {(
            [
              ["drag", t`Drag`],
              ["preset", t`Presets`],
              ["exact", t`Exact`],
            ] as const
          ).map(([value, label]) => (
            <button
              key={value}
              type="button"
              aria-pressed={mode === value}
              onClick={() => setMode(value)}
              className={toggle(mode === value)}
            >
              {label}
            </button>
          ))}
        </div>

        {source.parent_is_group && (
          <p className="rounded-lg border border-dashed p-3 text-xs text-muted-foreground">
            <Trans>
              It’s in the group {source.parent}: positions are measured from the group, so they’re
              only exact if the group fills the screen.
            </Trans>
          </p>
        )}
        {item.rotation !== 0 && (
          <p className="rounded-lg border border-dashed p-3 text-xs text-muted-foreground">
            <Trans>It’s turned in OBS, so the box is only roughly where it shows.</Trans>
          </p>
        )}
        {start.guessed && mode !== "preset" && (
          <p className="rounded-lg border border-dashed p-3 text-xs text-muted-foreground">
            <Trans>
              OBS doesn’t know its size yet (it isn’t showing anything), so the box starts in the
              middle.
            </Trans>
          </p>
        )}

        {/* On every tab: dragged on Drag, showing what the preset or numbers do on the others. */}
        {shown ? (
          <PlacementCanvas
            canvas={canvas}
            box={shown.box}
            content={shown.content}
            onChange={mode === "drag" ? setBox : undefined}
            name={source.name}
            others={others}
            background={still.url}
            keepShape={keepShape}
          />
        ) : (
          <p className="rounded-lg border border-dashed p-3 text-xs text-muted-foreground">
            <Trans>
              OBS doesn’t know its own size yet (it isn’t showing anything), so this can’t be drawn.
              It still works once it shows something.
            </Trans>
          </p>
        )}
        {!linesUp ? (
          <p className="text-xs text-muted-foreground">
            <Trans>
              It isn’t right on the scene on air, so the preview wouldn’t line up: the box is drawn
              without it.
            </Trans>
          </p>
        ) : (
          still.problem && <p className="text-xs text-muted-foreground">{still.problem}</p>
        )}

        {mode === "drag" && (
          <>
            <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
              <span className="font-mono tabular-nums">
                {Math.round(box.x)}, {Math.round(box.y)} · {Math.round(box.width)}×
                {Math.round(box.height)}
              </span>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => setBox(startingBox(item, canvas).box)}
              >
                <RotateCcw />
                <Trans>Undo changes</Trans>
              </Button>
            </div>
            <Toggle
              id={`${ids}-shape`}
              label={t`Keep its shape when resizing`}
              checked={keepShape}
              onChange={setKeepShape}
            />
          </>
        )}

        {mode === "preset" && (
          <>
            <fieldset className="flex flex-col gap-1.5">
              <legend className="mb-1.5 text-sm font-medium">
                <Trans>Size</Trans>
              </legend>
              <div className="grid grid-cols-4 gap-1 rounded-lg border p-0.5">
                {(Object.keys(fits) as Fit[]).map((value) => (
                  <button
                    key={value}
                    type="button"
                    aria-pressed={fit === value}
                    onClick={() => setFit(value)}
                    className={toggle(fit === value)}
                  >
                    {fits[value][0]}
                  </button>
                ))}
              </div>
              <p className="text-xs text-muted-foreground">{fits[fit][1]}</p>
            </fieldset>

            <div
              className={cn("flex flex-col gap-1.5", fit === "original" && "opacity-50")}
              aria-disabled={fit === "original"}
            >
              <div className="flex items-center justify-between text-sm font-medium">
                <span id={`${ids}-size`}>
                  <Trans>Of the screen</Trans>
                </span>
                <span className="font-mono tabular-nums">{size}%</span>
              </div>
              <Slider
                aria-labelledby={`${ids}-size`}
                aria-valuetext={`${size}%`}
                min={5}
                max={100}
                step={5}
                value={[size]}
                onValueChange={([value]) => value !== undefined && setSize(value)}
                disabled={fit === "original"}
              />
            </div>

            <fieldset className="flex flex-col gap-1.5">
              <legend className="mb-1.5 text-sm font-medium">
                <Trans>Where</Trans>
              </legend>
              {/* A small screen, each cell a spot to pin it to. */}
              <div className="grid aspect-video w-full max-w-64 grid-cols-3 grid-rows-3 gap-1 self-center rounded-lg border bg-surface-2/40 p-1">
                {ANCHORS.map((value) => (
                  <button
                    key={value}
                    type="button"
                    aria-pressed={anchor === value}
                    aria-label={labels[value]}
                    title={labels[value]}
                    onClick={() => setAnchor(value)}
                    className={cn(
                      "flex items-center justify-center rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
                      anchor === value ? "bg-primary" : "hover:bg-surface-2",
                    )}
                  >
                    <span
                      className={cn(
                        "size-2 rounded-full",
                        anchor === value ? "bg-primary-foreground" : "bg-muted-foreground/60",
                      )}
                    />
                  </button>
                ))}
              </div>
              <p className="text-center text-xs text-muted-foreground">{labels[anchor]}</p>
              {size === 100 && fit !== "original" && anchor !== "center" && (
                <p className="text-xs text-muted-foreground">
                  <Trans>
                    At 100% it fills the screen wherever it’s pinned. Make it smaller to put it in a
                    corner or on an edge.
                  </Trans>
                </p>
              )}
            </fieldset>

            <MoreSettings>
              <NumberField
                id={`${ids}-margin`}
                label={t`Margin from the edges (% of the screen’s height)`}
                value={margin}
                min={0}
                max={25}
                onChange={setMargin}
              />
              <p className="text-xs text-muted-foreground">
                <Trans>A full-screen box has none.</Trans>
              </p>
            </MoreSettings>
          </>
        )}

        {mode === "exact" && (
          <>
            <p className="text-xs text-muted-foreground">
              <Trans>
                A box in pixels on the {canvas.width}×{canvas.height} screen, from its top left. The
                source fits inside it, keeping its shape.
              </Trans>
            </p>
            <div className="grid grid-cols-2 gap-3">
              {(
                [
                  ["x", t`X`, -16384],
                  ["y", t`Y`, -16384],
                  ["width", t`Width`, 1],
                  ["height", t`Height`, 1],
                ] as const
              ).map(([key, label, min]) => (
                <NumberField
                  key={key}
                  id={`${ids}-${key}`}
                  label={label}
                  value={Math.round(box[key])}
                  min={min}
                  max={16384}
                  onChange={(value) => setBox((current) => ({ ...current, [key]: value }))}
                />
              ))}
            </div>
            <MoreSettings>
              <p className="text-xs text-muted-foreground">
                <Trans>Pixels cut off each side of the source, before it’s sized.</Trans>
              </p>
              <div className="grid grid-cols-2 gap-3">
                {(
                  [
                    ["left", t`Crop left`],
                    ["right", t`Crop right`],
                    ["top", t`Crop top`],
                    ["bottom", t`Crop bottom`],
                  ] as const
                ).map(([key, label]) => (
                  <NumberField
                    key={key}
                    id={`${ids}-crop-${key}`}
                    label={label}
                    value={crop[key]}
                    min={0}
                    max={16384}
                    onChange={(value) => setCrop((current) => ({ ...current, [key]: value }))}
                  />
                ))}
              </div>
            </MoreSettings>
          </>
        )}
      </div>
      <DialogFooter>
        <Button type="submit" disabled={save.isPending}>
          <Trans>Save</Trans>
        </Button>
      </DialogFooter>
    </form>
  );
}
