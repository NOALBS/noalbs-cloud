import {
  isValidElement,
  memo,
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import type { Layout, LayoutItem } from "react-grid-layout";
import GridLayout from "react-grid-layout/legacy";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useMediaQuery } from "@/hooks/use-media-query";
import { type FamilyVariants, type LiveVariant, paneFamily } from "./live-variants";
import "react-grid-layout/css/styles.css";
import "./pane-layout.css";

export const paneDefinitions = [
  { id: "status", title: "Status", minW: 1, minH: 3, w: 1, h: 4 },
  { id: "bitrate-chart", title: "Bitrate chart", minW: 1, minH: 3, w: 1, h: 4 },
  { id: "controls", title: "Actions", minW: 1, minH: 3, w: 1, h: 4 },
  { id: "servers", title: "Stream servers", minW: 1, minH: 3, w: 1, h: 4 },
  { id: "belabox", title: "BELABOX", minW: 1, minH: 3, w: 1, h: 4 },
  { id: "moblin", title: "Moblin", minW: 1, minH: 3, w: 1, h: 4 },
  { id: "preview", title: "OBS preview", minW: 1, minH: 3, w: 1, h: 4 },
  { id: "scenes", title: "Scenes", minW: 1, minH: 3, w: 1, h: 4 },
  { id: "sources", title: "Sources", minW: 1, minH: 3, w: 1, h: 4 },
  { id: "audio", title: "Audio", minW: 1, minH: 3, w: 1, h: 4 },
  { id: "activity", title: "Activity", minW: 1, minH: 2, w: 1, h: 4 },
  { id: "program-preview", title: "Program / Preview", minW: 1, minH: 3, w: 2, h: 4 },
  { id: "auto-switch", title: "Auto switch", minW: 1, minH: 3, w: 1, h: 4 },
] as const;
export type PaneId = (typeof paneDefinitions)[number]["id"];
type Saved = {
  visible: PaneId[];
  layouts: Record<string, Layout>;
  visibleByCols?: Record<string, PaneId[]>;
};
const streamer: PaneId[] = [
  "status",
  "controls",
  "scenes",
  "servers",
  "belabox",
  "moblin",
  "sources",
  "audio",
  "activity",
];
export const operator: PaneId[] = [
  "program-preview",
  "bitrate-chart",
  "servers",
  "belabox",
  "moblin",
  "scenes",
  "activity",
  "sources",
  "audio",
];
const allIds: string[] = paneDefinitions.map((p) => p.id);
export const storageKey = (id: string, phone: boolean, mode = true, variant: LiveVariant = "c") =>
  `noalbs-live-layout:v${mode ? 12 : 11}:${id}:${phone ? "phone" : "desktop"}:${mode ? "operator:research" : `streamer:${variant}`}`;
export const rowHeight = (availableHeight: number) => (availableHeight - 11 * 12) / 12;

export function operatorFor(cols: number): PaneId[] {
  return cols === 3
    ? [...operator.filter((id) => !["audio", "belabox", "moblin"].includes(id)), "controls"]
    : operator;
}

export function defaults(ids: PaneId[], cols: number): Layout {
  const positions: Partial<Record<PaneId, [number, number, number, number]>> =
    cols >= 5
      ? {
          status: [0, 0, 4, 2],
          controls: [0, 2, 4, 3],
          sources: [0, 8, 4, 4],
          servers: [16, 0, 4, 3],
          scenes: [0, 5, 16, 3],
          audio: [4, 8, 4, 4],
          belabox: [8, 8, 4, 4],
          moblin: [12, 8, 4, 4],
          activity: [16, 6, 4, 6],
          "bitrate-chart": [16, 3, 4, 3],
          preview: [8, 5, 8, 7],
          "program-preview": [0, 0, 16, 5],
        }
      : cols >= 4
        ? {
            status: [0, 0, 4, 3],
            controls: [0, 3, 4, 4],
            servers: [12, 0, 4, 3],
            "bitrate-chart": [12, 3, 4, 3],
            scenes: [0, 5, 12, 3],
            sources: [0, 8, 4, 4],
            preview: [8, 0, 4, 5],
            audio: [4, 8, 4, 4],
            activity: [12, 6, 4, 6],
            belabox: [8, 8, 4, 2],
            moblin: [8, 10, 4, 2],
            "program-preview": [0, 0, 12, 5],
          }
        : cols >= 3
          ? {
              status: [0, 0, 4, 4],
              scenes: [0, 4, 8, 3],
              "bitrate-chart": [8, 3, 4, 3],
              controls: [4, 7, 4, 5],
              sources: [0, 7, 4, 5],
              servers: [8, 0, 4, 3],
              activity: [8, 6, 4, 6],
              preview: [8, 3, 4, 5],
              "program-preview": [0, 0, 8, 4],
            }
          : {
              status: [0, 0, 4, 2],
              controls: [0, 2, 4, 2],
              servers: [0, 4, 4, 2],
              belabox: [0, 6, 4, 4],
              activity: [0, 10, 4, 2],
              "bitrate-chart": [4, 0, 4, 2],
              moblin: [4, 2, 4, 2],
              preview: [4, 4, 4, 2],
              scenes: [4, 6, 4, 2],
              sources: [4, 8, 4, 2],
              audio: [4, 10, 4, 2],
              "program-preview": [0, 6, 4, 4],
            };
  return ids.map((id, index) => {
    const p = paneDefinitions.find((p) => p.id === id) ?? paneDefinitions[0];
    const [x, y, w, h] = positions[id] ?? [0, 12 + index * 4, 4, p.h];
    return { i: id, x: x / 4, y, w: w / 4, h, minW: p.minW, minH: Math.min(p.minH, h) };
  });
}

export function read(key: string, phone: boolean, mode = true, variant: LiveVariant = "c"): Saved {
  try {
    const value = JSON.parse(localStorage.getItem(key) ?? "null");
    if (
      value &&
      Array.isArray(value.visible) &&
      value.visible.every((id: string) => allIds.includes(id)) &&
      new Set(value.visible).size === value.visible.length &&
      (value.visibleByCols === undefined ||
        (value.visibleByCols &&
          typeof value.visibleByCols === "object" &&
          Object.values(value.visibleByCols).every(
            (panes) =>
              Array.isArray(panes) &&
              panes.every((id: string) => allIds.includes(id)) &&
              new Set(panes).size === panes.length,
          ))) &&
      value.layouts &&
      typeof value.layouts === "object" &&
      Object.values(value.layouts).every(
        (layout) =>
          Array.isArray(layout) &&
          layout.every(
            (p: LayoutItem) =>
              p &&
              allIds.includes(p.i) &&
              [p.x, p.y, p.w, p.h].every(Number.isFinite) &&
              p.x >= 0 &&
              p.y >= 0 &&
              p.w > 0 &&
              p.h > 0,
          ),
      )
    )
      return value;
  } catch {
    /* Invalid or unavailable storage uses the preset. */
  }
  return modeSaved(phone, mode, variant);
}

export const operatorColumns = (cols: number) => (cols >= 4 ? 5 : cols);

export function researchOperatorSaved(): Saved {
  const layouts: Record<string, Layout> = {};
  const visibleByCols: Record<string, PaneId[]> = {};
  for (const cols of [2, 3, 4, 5]) {
    const main = operatorColumns(cols) - 1;
    const compact = cols < 4;
    const definitions: [PaneId, number, number, number, number][] = [
      ["program-preview", 0, 0, main, compact ? 6 : 5],
      ["scenes", 0, compact ? 6 : 5, main, compact ? 3 : 2],
      ["sources", 0, compact ? 9 : 7, compact ? main : 1, compact ? 3 : 5],
      ["servers", main, 0, 1, 3],
      ["auto-switch", main, 3, 1, compact ? 5 : 4],
      ["activity", main, compact ? 8 : 7, 1, compact ? 4 : 5],
      ...(!compact
        ? ([
            ["audio", 1, 7, 1, 5],
            ["belabox", 2, 7, 1, 5],
            ["moblin", 3, 7, 1, 5],
          ] as [PaneId, number, number, number, number][])
        : []),
    ];
    layouts[cols] = definitions.map(([i, x, y, w, h]) => ({
      i,
      x,
      y,
      w,
      h,
      minW: 1,
      minH: Math.min(3, h),
    }));
    visibleByCols[cols] = definitions.map(([id]) => id);
  }
  return { visible: visibleByCols[4] ?? [], visibleByCols, layouts };
}

export function modeSaved(phone: boolean, mode: boolean, variant: LiveVariant): Saved {
  if (phone) return { visible: streamer, layouts: {} };
  if (mode) return researchOperatorSaved();
  const layouts: Record<string, Layout> = {};
  const visibleByCols: Record<string, PaneId[]> = {};
  for (const cols of [2, 3, 4, 5]) {
    const last = cols - 1;
    const definitions: [PaneId, number, number, number, number][] =
      variant === "c"
        ? [
            ["preview", 0, 0, last, 5],
            ["scenes", 0, 5, last, 3],
            ["controls", 0, 8, 1, 4],
            ["sources", Math.min(1, last), 8, Math.max(1, last - 1), 4],
            ["servers", last, 0, 1, 4],
            ["activity", last, 4, 1, 8],
          ]
        : cols >= 4
          ? [
              ["status", 0, 0, 1, 4],
              ["preview", 0, 4, 1, 5],
              ["servers", 0, 9, 1, 3],
              ["controls", 1, 0, 1, 6],
              ["sources", 1, 6, 1, 6],
              ["scenes", 2, 0, cols - 2, 6],
              ["activity", 2, 6, 1, 6],
              ["bitrate-chart", 3, 6, cols - 3, 6],
            ]
          : [
              ["status", 0, 0, 1, 4],
              ["servers", 0, 4, 1, 4],
              ["preview", 0, 8, 1, 4],
              ["controls", 1, 0, 1, 6],
              ["sources", 1, 6, 1, 6],
              ...(cols === 3
                ? ([
                    ["scenes", 2, 0, 1, 6],
                    ["activity", 2, 6, 1, 6],
                  ] as [PaneId, number, number, number, number][])
                : []),
            ];
    layouts[cols] = definitions.map(([i, x, y, w, h]) => ({
      i,
      x,
      y,
      w,
      h,
      minW: 1,
      minH: Math.min(3, h),
    }));
    visibleByCols[cols] = definitions.map(([id]) => id);
  }
  return { visible: visibleByCols[4] ?? [], visibleByCols, layouts };
}

export function PaneLayout({
  switcherId,
  panes,
  header,
  operatorMode = false,
  variant = "a",
  designs,
}: {
  switcherId: string;
  panes: Record<PaneId, ReactNode>;
  header: HTMLElement | null;
  operatorMode?: boolean;
  variant?: LiveVariant;
  designs?: FamilyVariants;
}) {
  const phone = !useMediaQuery("(min-width: 768px)");
  return (
    <LayoutEditor
      key={`${switcherId}-${phone}-${operatorMode}-${variant}`}
      switcherId={switcherId}
      phone={phone}
      panes={panes}
      header={header}
      operatorMode={operatorMode}
      variant={variant}
      designs={designs}
    />
  );
}

export function samePaneBody(previous: ReactNode, next: ReactNode): boolean {
  if (Object.is(previous, next)) return true;
  if (
    !isValidElement(previous) ||
    !isValidElement(next) ||
    previous.type !== next.type ||
    previous.key !== next.key
  )
    return false;
  const before = previous.props as Record<string, unknown>;
  const after = next.props as Record<string, unknown>;
  return (
    Object.keys(before).length === Object.keys(after).length &&
    Object.keys(before).every((key) => {
      if (Object.is(before[key], after[key])) return true;
      const a = before[key];
      const b = after[key];
      return (
        a != null &&
        b != null &&
        typeof a === "object" &&
        typeof b === "object" &&
        Object.getPrototypeOf(a) === Object.prototype &&
        Object.getPrototypeOf(b) === Object.prototype &&
        Object.keys(a).length === Object.keys(b).length &&
        Object.entries(a).every(([name, value]) =>
          Object.is(value, (b as Record<string, unknown>)[name]),
        )
      );
    })
  );
}

const PaneBody = memo(
  function PaneBody({ children }: { children: ReactNode; frozen: boolean }) {
    return children;
  },
  (previous, next) => next.frozen || samePaneBody(previous.children, next.children),
);

function PaneFrame({
  id,
  editing,
  tools,
  children,
}: {
  id: PaneId;
  editing: boolean;
  tools: ReactNode;
  children: ReactNode;
}) {
  const content = useRef<HTMLDivElement>(null);
  const [heading, setHeading] = useState<HTMLElement | null>(null);
  useEffect(() => {
    const title = content.current?.querySelector<HTMLElement>("h2, [data-pane-title]");
    if (!title) return;
    title.dataset.paneHeading = "true";
    const row = title.hasAttribute("data-pane-title")
      ? title.parentElement?.parentElement
      : title.parentElement?.tagName === "DIV"
        ? title.parentElement
        : title;
    if (!row) return;
    row.classList.add("pane-native-header");
    setHeading(row);
  }, []);
  const actions = editing && (
    <span className="pane-tools ml-auto inline-flex shrink-0 items-center gap-1 font-normal">
      {tools}
    </span>
  );
  return (
    <section
      className={`pane-frame relative flex h-full min-h-0 flex-col overflow-hidden rounded-2xl border bg-card ${editing ? "border-primary" : ""}`}
    >
      {!heading && (
        <header className="pane-header flex h-11 shrink-0 items-center gap-2 px-3">
          <h2 data-pane-heading className="truncate text-sm font-semibold">
            {paneDefinitions.find((p) => p.id === id)?.title}
          </h2>
          {actions}
        </header>
      )}
      <div
        ref={content}
        className={`pane-content min-h-0 flex-1 overflow-auto px-3 pb-2 ${heading ? "pane-with-header" : ""}`}
      >
        {children}
      </div>
      {heading && createPortal(actions, heading)}
    </section>
  );
}

function LayoutEditor({
  switcherId,
  phone,
  panes,
  header,
  operatorMode,
  variant,
  designs,
}: {
  switcherId: string;
  phone: boolean;
  panes: Record<PaneId, ReactNode>;
  header: HTMLElement | null;
  operatorMode: boolean;
  variant: LiveVariant;
  designs?: FamilyVariants;
}) {
  const key = storageKey(switcherId, phone, operatorMode, variant);
  const [saved, setSaved] = useState<Saved>(() => read(key, phone, operatorMode, variant));
  const [editing, setEditing] = useState(false);
  const [interacting, setInteracting] = useState(false);
  const snapshot = useRef(panes);
  if (!interacting) snapshot.current = panes;
  const paneNodes = snapshot.current;
  const container = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 600 });
  const medium = useMediaQuery("(min-width: 1200px)");
  const large = useMediaQuery("(min-width: 1800px)");
  const wide = useMediaQuery("(min-width: 2400px)");
  const cols = wide ? 5 : large ? 4 : medium ? 3 : 2;
  const visible = useMemo(
    () =>
      (saved.visibleByCols?.[cols] ?? saved.visible).filter(
        (id) => operatorMode || !["program-preview", "auto-switch"].includes(id),
      ),
    [saved, cols, operatorMode],
  );
  const layout = useMemo(
    () => saved.layouts[cols] ?? defaults(visible, cols),
    [saved.layouts, visible, cols],
  );
  useEffect(() => {
    const element = container.current;
    if (!element) return;
    const measure = () => {
      const rect = element.getBoundingClientRect();
      setSize((current) => {
        const next = {
          width: rect.width,
          height: Math.max(240, window.innerHeight - rect.top - 8),
        };
        return next.width === current.width && next.height === current.height ? current : next;
      });
    };
    const observer = new ResizeObserver(measure);
    observer.observe(element.parentElement ?? element);
    window.addEventListener("resize", measure);
    measure();
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, []);
  useEffect(() => {
    try {
      localStorage.setItem(key, JSON.stringify(saved));
    } catch {
      /* Layout remains usable in memory. */
    }
  }, [key, saved]);
  const update = (next: Layout) => {
    setInteracting(false);
    setSaved((s) => ({ ...s, layouts: { ...s.layouts, [cols]: next } }));
  };
  const withVisible = useCallback(
    (s: Saved, next: PaneId[]): Saved =>
      phone
        ? { ...s, visible: next }
        : { ...s, visibleByCols: { ...s.visibleByCols, [cols]: next } },
    [phone, cols],
  );
  const hide = useCallback(
    (id: PaneId) =>
      setSaved((s) => ({
        ...withVisible(
          s,
          visible.filter((p) => p !== id),
        ),
        layouts: {
          ...s.layouts,
          [cols]: (s.layouts[cols] ?? defaults(visible, cols)).filter((p) => p.i !== id),
        },
      })),
    [withVisible, visible, cols],
  );
  const add = (id: PaneId) =>
    setSaved((s) => {
      const items = s.layouts[cols] ?? defaults(visible, cols);
      const candidate = defaults([id], cols)[0] as LayoutItem;
      const overlaps = items.some(
        (item) =>
          item.x < candidate.x + candidate.w &&
          item.x + item.w > candidate.x &&
          item.y < candidate.y + candidate.h &&
          item.y + item.h > candidate.y,
      );
      return {
        ...withVisible(s, [...visible, id]),
        layouts: {
          ...s.layouts,
          [cols]: [
            ...items,
            {
              ...candidate,
              y: overlaps ? Math.max(0, ...items.map((p) => p.y + p.h)) : candidate.y,
            },
          ],
        },
      };
    });
  const move = useCallback(
    (index: number, direction: number) =>
      setSaved((s) => {
        const visible = [...s.visible];
        const current = visible[index];
        const next = visible[index + direction];
        if (!current || !next) return s;
        visible[index] = next;
        visible[index + direction] = current;
        return { ...s, visible };
      }),
    [],
  );
  const toolbar = (
    <div className="flex flex-wrap items-center gap-2">
      {editing && (
        <>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline">Add pane</Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent side="bottom" align="start" sideOffset={8}>
              {paneDefinitions
                .filter(
                  (p) =>
                    !visible.includes(p.id) &&
                    (operatorMode || !["program-preview", "auto-switch"].includes(p.id)),
                )
                .map((p) => (
                  <DropdownMenuItem key={p.id} onSelect={() => add(p.id)}>
                    {p.title}
                  </DropdownMenuItem>
                ))}
              {visible.length === paneDefinitions.length && (
                <DropdownMenuItem disabled>All panes visible</DropdownMenuItem>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline">Reset layout</Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent side="bottom" align="start" sideOffset={8}>
              {(operatorMode ? (["Operator", "Streamer"] as const) : (["Streamer"] as const)).map(
                (preset) => (
                  <DropdownMenuItem
                    key={preset}
                    onSelect={() => setSaved(modeSaved(phone, preset === "Operator", variant))}
                  >
                    {preset}
                  </DropdownMenuItem>
                ),
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        </>
      )}
      <Button variant="outline" onClick={() => setEditing(!editing)}>
        {editing ? "Done" : "Customize"}
      </Button>
    </div>
  );
  const content = useCallback(
    (id: PaneId, index: number) => (
      <PaneFrame
        id={id}
        editing={editing}
        tools={
          <>
            {phone ? (
              <>
                <Button
                  size="sm"
                  variant="ghost"
                  aria-label={`Move ${id} up`}
                  disabled={index === 0}
                  onClick={() => move(index, -1)}
                >
                  ↑
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  aria-label={`Move ${id} down`}
                  disabled={index === visible.length - 1}
                  onClick={() => move(index, 1)}
                >
                  ↓
                </Button>
              </>
            ) : (
              <span className="pane-drag-handle cursor-grab touch-none px-1" title={`Drag ${id}`}>
                ⠿
              </span>
            )}
            <Button size="sm" variant="ghost" aria-label={`Hide ${id}`} onClick={() => hide(id)}>
              Hide
            </Button>
          </>
        }
      >
        <PaneBody frozen={interacting}>{paneNodes[id]}</PaneBody>
      </PaneFrame>
    ),
    [editing, phone, move, hide, visible.length, interacting, paneNodes],
  );
  const gridChildren = useMemo(
    () =>
      visible.map((id, i) => (
        <div key={id} data-pane={id} data-design={designs?.[paneFamily(id) ?? "scenes"] ?? variant}>
          {content(id, i)}
        </div>
      )),
    [visible, content, designs, variant],
  );
  return (
    <div
      ref={container}
      data-testid="live-pane-layout"
      data-interacting={interacting}
      data-editing={editing}
      className={phone ? "" : "overflow-auto"}
      style={phone ? undefined : { height: size.height, flexShrink: 0 }}
    >
      {header ? createPortal(toolbar, header) : toolbar}
      {phone ? (
        <div className="flex flex-col gap-3">{gridChildren}</div>
      ) : (
        size.width > 0 && (
          <GridLayout
            width={size.width}
            cols={operatorMode ? operatorColumns(cols) : cols}
            rowHeight={rowHeight(size.height)}
            margin={[12, 12]}
            containerPadding={[0, 0]}
            layout={layout}
            compactType="vertical"
            isDraggable={editing}
            isResizable={editing}
            draggableHandle=".pane-drag-handle"
            onDragStart={() => setInteracting(true)}
            onResizeStart={() => setInteracting(true)}
            onDragStop={update}
            onResizeStop={update}
          >
            {gridChildren}
          </GridLayout>
        )
      )}
      {visible.length === 0 && (
        <p className="py-10 text-center text-muted-foreground">
          No panes visible. Customize to add a pane or reset your layout.
        </p>
      )}
    </div>
  );
}
