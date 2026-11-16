import { ArrowDown, ArrowUp, LayoutDashboard, X } from "lucide-react";
import { memo, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { Layout } from "react-grid-layout";
import GridLayout from "react-grid-layout/legacy";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useMediaQuery } from "@/hooks/use-media-query";
import "react-grid-layout/css/styles.css";
import "./live.css";

export const paneDefinitions = [
  { id: "monitors", title: "Preview / Program", min: 620, operator: true },
  { id: "preview", title: "Preview", min: 300 },
  { id: "on-air", title: "On air", min: 220 },
  { id: "controls", title: "Actions", min: 300 },
  { id: "bitrate", title: "Bitrate", min: 300 },
  { id: "scenes", title: "Scenes", min: 320 },
  { id: "sources", title: "Sources", min: 280 },
  { id: "audio", title: "Audio", min: 300 },
  { id: "servers", title: "Stream servers", min: 320 },
  { id: "devices", title: "Devices", min: 340 },
  { id: "belabox", title: "BELABOX", min: 340 },
  { id: "moblin", title: "Moblin", min: 340 },
  { id: "obs", title: "OBS", min: 260 },
  { id: "activity", title: "Activity", min: 300 },
] as const;
export type PaneId = (typeof paneDefinitions)[number]["id"];
export type Mode = "streamer" | "operator";
type Spot = [id: PaneId, x: number, y: number, w: number, h: number];
type Saved = { phone?: PaneId[]; grids: Record<string, Layout> };

const ids: string[] = paneDefinitions.map((pane) => pane.id);
const ROW = 4;
const GAP = 16;
const MIN_H = 6;

/** Grid columns for a content width: wider screens get more columns, not wider panes. */
/** Below this content width the panes stack in one column, as on a phone. */
const STACK = 700;
export const columnsFor = (width: number) => (width >= 2100 ? 36 : width >= 1500 ? 24 : 12);

const presets: Record<Mode, Record<number, Spot[]>> = {
  operator: {
    12: [
      ["monitors", 0, 0, 12, 20],
      ["scenes", 0, 20, 12, 9],
      ["bitrate", 0, 29, 6, 13],
      ["servers", 6, 29, 6, 13],
      ["sources", 0, 42, 6, 18],
      ["audio", 6, 42, 6, 18],
      ["devices", 0, 60, 6, 24],
      ["obs", 6, 60, 6, 24],
      ["activity", 0, 84, 12, 16],
    ],
    24: [
      ["monitors", 0, 0, 18, 23],
      ["scenes", 0, 23, 18, 9],
      ["sources", 0, 32, 5, 22],
      ["audio", 5, 32, 6, 22],
      ["devices", 11, 32, 7, 22],
      ["bitrate", 18, 0, 6, 13],
      ["servers", 18, 13, 6, 12],
      ["obs", 18, 25, 6, 22],
      ["activity", 18, 47, 6, 16],
    ],
    36: [
      ["monitors", 0, 0, 20, 26],
      ["scenes", 0, 26, 20, 9],
      ["sources", 0, 35, 6, 22],
      ["audio", 6, 35, 7, 22],
      ["obs", 13, 35, 7, 22],
      ["belabox", 20, 0, 8, 26],
      ["moblin", 20, 26, 8, 31],
      ["bitrate", 28, 0, 8, 13],
      ["servers", 28, 13, 8, 12],
      ["activity", 28, 25, 8, 32],
    ],
  },
  streamer: {
    12: [
      ["preview", 0, 0, 6, 15],
      ["on-air", 6, 0, 6, 15],
      ["bitrate", 0, 15, 6, 13],
      ["servers", 6, 15, 6, 13],
      ["scenes", 0, 28, 12, 9],
      ["sources", 0, 37, 6, 20],
      ["audio", 6, 37, 6, 20],
      ["obs", 0, 57, 6, 22],
      ["activity", 6, 57, 6, 22],
      ["belabox", 0, 79, 6, 32],
      ["moblin", 6, 79, 6, 32],
    ],
    24: [
      ["preview", 0, 0, 7, 15],
      ["on-air", 7, 0, 4, 15],
      ["bitrate", 11, 0, 7, 15],
      ["scenes", 0, 15, 18, 9],
      ["sources", 0, 24, 9, 20],
      ["audio", 9, 24, 9, 20],
      ["belabox", 0, 44, 9, 32],
      ["moblin", 9, 44, 9, 32],
      ["servers", 18, 0, 6, 12],
      ["obs", 18, 12, 6, 22],
      ["activity", 18, 34, 6, 42],
    ],
    36: [
      ["preview", 0, 0, 10, 17],
      ["on-air", 10, 0, 6, 17],
      ["bitrate", 16, 0, 12, 17],
      ["scenes", 0, 17, 28, 9],
      ["sources", 0, 26, 7, 32],
      ["audio", 7, 26, 7, 32],
      ["belabox", 14, 26, 7, 32],
      ["moblin", 21, 26, 7, 32],
      ["servers", 28, 0, 8, 12],
      ["obs", 28, 12, 8, 22],
      ["activity", 28, 34, 8, 24],
    ],
  },
};
const phonePreset: PaneId[] = [
  "bitrate",
  "controls",
  "scenes",
  "servers",
  "belabox",
  "moblin",
  "sources",
  "audio",
  "activity",
];

export const storageKey = (switcherId: string, mode: Mode) =>
  `noalbs-live-layout:v${mode === "streamer" ? 22 : 21}:${switcherId}:${mode}`;

export function preset(mode: Mode, cols: number): Layout {
  return (presets[mode][cols] ?? []).map(([i, x, y, w, h]) => ({
    i,
    x,
    y,
    w,
    h,
    minH: MIN_H,
  }));
}

export function fitPreset(layout: Layout, needs: Record<string, number>): Layout {
  const growth: { at: number; by: number }[] = [];
  const before = (y: number) => growth.reduce((sum, row) => sum + (row.at <= y ? row.by : 0), 0);
  for (const item of [...layout].sort((a, b) => a.y + a.h - (b.y + b.h))) {
    const end = item.y + item.h;
    const height = item.h + before(end) - before(item.y);
    const extra = (needs[item.i] ?? 0) - height;
    if (Number.isFinite(extra) && extra > 0) growth.push({ at: end, by: extra });
  }
  return layout.map((item) => ({
    ...item,
    y: item.y + before(item.y),
    h: item.h + before(item.y + item.h) - before(item.y),
  }));
}

export function read(key: string): Saved {
  try {
    const value = JSON.parse(localStorage.getItem(key) ?? "null");
    const known = (list: unknown) =>
      Array.isArray(list) &&
      list.every((id) => ids.includes(id)) &&
      new Set(list).size === list.length;
    if (
      value &&
      (value.phone === undefined || known(value.phone)) &&
      value.grids &&
      typeof value.grids === "object" &&
      Object.values(value.grids).every(
        (layout) =>
          Array.isArray(layout) &&
          known(layout.map((item) => item?.i)) &&
          layout.every(
            (item) =>
              [item.x, item.y, item.w, item.h].every(Number.isFinite) &&
              item.x >= 0 &&
              item.y >= 0 &&
              item.w > 0 &&
              item.h > 0,
          ),
      )
    )
      return value;
  } catch {
    /* Invalid or unavailable storage uses the preset. */
  }
  return { grids: {} };
}

const title = (id: PaneId) => paneDefinitions.find((pane) => pane.id === id)?.title ?? id;

/** Holds a pane's content still while a pane is dragged or resized, so live data doesn't re-render it mid-gesture. */
const PaneBody = memo(
  function PaneBody({ children }: { children: ReactNode; frozen: { current: boolean } }) {
    return children;
  },
  (_, next) => next.frozen.current,
);

/**
 * The Live page's panes. On desktop a grid the user can rearrange, resize and
 * add to; on phones one column they can reorder. The page scrolls: a layout can
 * be taller than the window. Saved in the browser per switcher and mode.
 * A pane whose content is `null` isn't offered (e.g. BELABOX when not set up).
 */
export function PaneLayout({
  switcherId,
  mode,
  panes,
  toolbar,
}: {
  switcherId: string;
  mode: Mode;
  panes: Record<PaneId, ReactNode>;
  /** Where the Edit layout button goes (the page header). */
  toolbar: HTMLElement | null;
}) {
  const small = !useMediaQuery("(min-width: 768px)");
  const key = storageKey(switcherId, mode);
  const [saved, setSaved] = useState<{ key: string; value: Saved }>(() => ({
    key,
    value: read(key),
  }));
  if (saved.key !== key) setSaved({ key, value: read(key) });
  const [editing, setEditing] = useState(false);
  // A ref, not state: starting a drag must not re-render the page.
  const interacting = useRef(false);
  const setInteracting = (value: boolean) => {
    interacting.current = value;
    if (container.current) container.current.dataset.interacting = String(value);
  };
  const container = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    const element = container.current;
    if (!element) return;
    const observer = new ResizeObserver(() => setWidth(element.clientWidth));
    observer.observe(element);
    setWidth(element.clientWidth);
    return () => observer.disconnect();
  }, []);
  const cols = columnsFor(width);
  const phone = small || (width > 0 && width < STACK);
  // A pane is never shorter than its content: the page scrolls instead.
  const [needs, setNeeds] = useState<Record<string, number>>({});
  const fitKey = `${key}:${cols}:${width}`;
  const fitted = useRef(fitKey);
  if (fitted.current !== fitKey) {
    fitted.current = fitKey;
    if (Object.keys(needs).length) setNeeds({});
  }
  useEffect(() => {
    if (phone) return;
    const measure = () => {
      if (!container.current || interacting.current) return;
      const grown: Record<string, number> = {};
      for (const pane of container.current?.querySelectorAll<HTMLElement>("[data-pane]") ?? []) {
        const content = pane.firstElementChild;
        const over = content ? content.scrollHeight - content.clientHeight : 0;
        if (over > 1)
          grown[pane.dataset.pane ?? ""] = Math.ceil(
            (pane.offsetHeight + over + GAP) / (ROW + GAP),
          );
      }
      if (Object.keys(grown).length) setNeeds((current) => ({ ...current, ...grown }));
    };
    measure();
    const timer = setInterval(measure, 500);
    return () => clearInterval(timer);
  }, [phone]);
  const change = (next: (value: Saved) => Saved) =>
    setSaved((current) => {
      const value = next(current.value);
      try {
        localStorage.setItem(current.key, JSON.stringify(value));
      } catch {
        /* The layout stays usable in memory. */
      }
      return { key: current.key, value };
    });

  const offered = (id: PaneId) =>
    panes[id] != null &&
    (mode === "operator" || !paneDefinitions.find((pane) => pane.id === id && "operator" in pane));
  const offeredKey = paneDefinitions.map((pane) => (offered(pane.id) ? pane.id : "")).join();
  // biome-ignore lint/correctness/useExhaustiveDependencies: `offeredKey` stands in for `offered`
  const layout = useMemo(
    () =>
      (saved.value.grids[cols] ?? preset(mode, cols)).filter((item) => offered(item.i as PaneId)),
    [saved.value.grids, cols, mode, offeredKey],
  );
  const fitRows = mode === "streamer" && !saved.value.grids[cols];
  const fittedLayout = useMemo(
    () =>
      (fitRows ? fitPreset(layout, needs) : layout).map((item) => {
        // The fewest columns that keep the pane's content from being squeezed.
        const column = (width - GAP * (cols - 1)) / cols;
        const px = paneDefinitions.find((pane) => pane.id === item.i)?.min ?? 0;
        const minW = Math.min(cols, Math.max(1, Math.ceil((px + GAP) / (column + GAP))));
        const minH = Math.max(MIN_H, needs[item.i] ?? 0);
        // The minimum limits resizing; it never widens a pane already placed, which would push others around.
        return { ...item, h: Math.max(item.h, minH), minW: Math.min(minW, item.w), minH };
      }),
    [layout, needs, width, cols, fitRows],
  );
  const order = (
    phone ? (saved.value.phone ?? phonePreset) : layout.map((item) => item.i as PaneId)
  ).filter(offered);
  const hidden = paneDefinitions
    .map((pane) => pane.id)
    .filter((id) => offered(id) && !order.includes(id));

  const setGrid = (next: Layout) =>
    change((value) => ({ ...value, grids: { ...value.grids, [cols]: next } }));
  const hide = (id: PaneId) =>
    phone
      ? change((value) => ({ ...value, phone: order.filter((pane) => pane !== id) }))
      : setGrid(layout.filter((item) => item.i !== id));
  const add = (id: PaneId) => {
    if (phone) return change((value) => ({ ...value, phone: [...order, id] }));
    const spot = [...Object.values(presets[mode]), ...Object.values(presets.operator)]
      .flat()
      .find(([pane]) => pane === id);
    const w = Math.min(spot?.[3] ?? 6, cols);
    setGrid([
      ...layout,
      {
        i: id,
        x: 0,
        y: Math.max(0, ...layout.map((item) => item.y + item.h)),
        w,
        h: spot?.[4] ?? 16,
        minH: MIN_H,
      },
    ]);
  };
  const move = (index: number, by: number) => {
    const next = [...order];
    const [pane] = next.splice(index, 1);
    if (pane) next.splice(index + by, 0, pane);
    change((value) => ({ ...value, phone: next }));
  };
  const reset = () =>
    change((value) =>
      phone
        ? { ...value, phone: undefined }
        : {
            ...value,
            grids: Object.fromEntries(
              Object.entries(value.grids).filter(([c]) => c !== String(cols)),
            ),
          },
    );

  const tools = (
    <>
      {editing && (
        <>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="sm" disabled={hidden.length === 0}>
                Add pane
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {hidden.map((id) => (
                <DropdownMenuItem key={id} onSelect={() => add(id)}>
                  {title(id)}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
          <Button variant="outline" size="sm" onClick={reset}>
            Reset
          </Button>
        </>
      )}
      <Button
        variant={editing ? "default" : "outline"}
        size="sm"
        onClick={() => setEditing(!editing)}
      >
        {!editing && <LayoutDashboard />}
        {editing ? "Done" : "Edit layout"}
      </Button>
    </>
  );

  const frame = (id: PaneId, index: number) => (
    <div key={id}>
      <div data-pane={id} className="live-pane">
        <div className="live-pane-fit">
          <PaneBody frozen={interacting}>{panes[id]}</PaneBody>
        </div>
        {editing && (
          <div className="live-pane-edit">
            <span className="live-pane-edit-bar">
              <span className="px-1.5 text-xs font-semibold">{title(id)}</span>
              {phone && (
                <>
                  <Button
                    size="icon-sm"
                    variant="ghost"
                    aria-label={`Move ${title(id)} up`}
                    disabled={index === 0}
                    onClick={() => move(index, -1)}
                  >
                    <ArrowUp />
                  </Button>
                  <Button
                    size="icon-sm"
                    variant="ghost"
                    aria-label={`Move ${title(id)} down`}
                    disabled={index === order.length - 1}
                    onClick={() => move(index, 1)}
                  >
                    <ArrowDown />
                  </Button>
                </>
              )}
              <Button
                size="icon-sm"
                variant="ghost"
                aria-label={`Hide ${title(id)}`}
                onMouseDown={(event) => event.stopPropagation()}
                onClick={() => hide(id)}
              >
                <X />
              </Button>
            </span>
          </div>
        )}
      </div>
    </div>
  );

  return (
    <div ref={container} data-testid="live-pane-layout" data-editing={editing}>
      {toolbar ? (
        createPortal(tools, toolbar)
      ) : (
        <div className="mb-3 flex justify-end gap-2">{tools}</div>
      )}
      {phone ? (
        <div className="flex flex-col gap-3">{order.map(frame)}</div>
      ) : (
        width > 0 && (
          <GridLayout
            width={width}
            cols={cols}
            rowHeight={ROW}
            margin={[GAP, GAP]}
            containerPadding={[0, 0]}
            layout={fittedLayout}
            compactType="vertical"
            isDraggable={editing}
            isResizable={editing}
            draggableCancel="button"
            onDragStart={() => setInteracting(true)}
            onResizeStart={() => setInteracting(true)}
            onDragStop={(next) => {
              setInteracting(false);
              setGrid(next);
            }}
            resizeHandles={["se", "sw", "e", "w", "s"]}
            onResizeStop={(next, before) => {
              setInteracting(false);
              // A pane made wider may need less height: measure it again.
              const resized = next.find((item) => item.i === before?.i);
              if (resized && resized.w !== before?.w)
                setNeeds(({ [resized.i]: _, ...rest }) => rest);
              setGrid(next);
            }}
          >
            {order.map(frame)}
          </GridLayout>
        )
      )}
      {order.length === 0 && (
        <p className="py-10 text-center text-muted-foreground">
          No panes shown. Use Edit layout to add one.
        </p>
      )}
    </div>
  );
}
