import {
  closestCenter,
  DndContext,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  rectSortingStrategy,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
} from "@dnd-kit/sortable";
import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { GripVertical, Minus, Plus, X } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { cn } from "@/lib/utils";
import {
  type Availability,
  addTile,
  type Columns,
  hiddenTiles,
  hideTile,
  minSpan,
  moveTile,
  type Preset,
  resizeTile,
  type Tile,
  tileNames,
} from "./layout";
import { TileEditing } from "./tile";
import type { useLiveLayout } from "./use-live-layout";

export function LiveLayoutEditor({
  layout,
  cols,
  desktop,
  availability,
  className,
  children,
}: {
  layout: ReturnType<typeof useLiveLayout>;
  cols: Columns;
  desktop: boolean;
  availability: Availability;
  className?: string;
  children: (tile: Tile) => ReactNode;
}) {
  const [editing, setEditing] = useState(false);
  const [adding, setAdding] = useState(false);
  const grid = useRef<HTMLDivElement>(null);
  const done = useRef<HTMLButtonElement>(null);
  const edit = useRef<HTMLButtonElement>(null);
  const wasEditing = useRef(false);
  const active = desktop && editing;
  const names = tileNames();
  const hidden = hiddenTiles(layout.tiles, availability);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );
  useEffect(() => {
    if (!desktop) {
      setEditing(false);
      setAdding(false);
    }
  }, [desktop]);
  useEffect(() => {
    if (active) done.current?.focus();
    else if (wasEditing.current && desktop) edit.current?.focus();
    wasEditing.current = active;
  }, [active, desktop]);
  return (
    <>
      {desktop && (
        <fieldset
          className="flex min-w-0 flex-wrap items-center justify-end gap-2 border-0 p-0"
          aria-label={t`Layout controls`}
        >
          {active ? (
            <>
              <label className="flex items-center gap-2 text-xs text-muted-foreground">
                <Trans>Preset</Trans>
                <select
                  aria-label={t`Layout preset`}
                  value={layout.preset}
                  onChange={(event) => layout.setPreset(event.target.value as Preset)}
                  className="h-8 rounded-lg border bg-card px-2 text-sm text-foreground"
                >
                  <option value="default">{t`Default`}</option>
                  <option value="operator">{t`Operator`}</option>
                  <option value="compact">{t`Compact`}</option>
                </select>
              </label>
              <Button variant="outline" size="sm" onClick={layout.reset}>
                <Trans>Reset to preset</Trans>
              </Button>
              <Popover open={adding} onOpenChange={setAdding}>
                <PopoverTrigger asChild>
                  <Button variant="outline" size="sm">
                    <Plus />
                    <Trans>Add panel</Trans>
                  </Button>
                </PopoverTrigger>
                <PopoverContent
                  align="end"
                  className="flex max-h-80 w-56 flex-col gap-1 overflow-y-auto p-2"
                >
                  {hidden.length ? (
                    hidden.map((id) => (
                      <Button
                        key={id}
                        variant="ghost"
                        size="sm"
                        className="justify-start"
                        onClick={() => {
                          layout.updateTiles((tiles) => addTile(tiles, id, cols, availability));
                          setAdding(false);
                        }}
                      >
                        {names[id]}
                      </Button>
                    ))
                  ) : (
                    <p className="px-2 py-3 text-sm text-muted-foreground">
                      <Trans>All panels are shown.</Trans>
                    </p>
                  )}
                </PopoverContent>
              </Popover>
              <Button
                ref={done}
                size="sm"
                onClick={() => {
                  setEditing(false);
                  setAdding(false);
                }}
              >
                <Trans>Done</Trans>
              </Button>
            </>
          ) : (
            <Button ref={edit} variant="ghost" size="sm" onClick={() => setEditing(true)}>
              <Trans>Edit layout</Trans>
            </Button>
          )}
        </fieldset>
      )}
      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        accessibility={{
          screenReaderInstructions: {
            draggable: t`To move a panel, press Space, then use the arrow keys. Press Space again to drop, or Escape to cancel.`,
          },
        }}
        onDragEnd={({ active: dragged, over }) => {
          if (!active || !over || dragged.id === over.id) return;
          layout.updateTiles((tiles) => {
            const id = tiles.find((tile) => tile.id === dragged.id)?.id;
            const index = tiles.findIndex((tile) => tile.id === over.id);
            return id && index >= 0 ? moveTile(tiles, id, index) : tiles;
          });
        }}
      >
        <SortableContext items={layout.tiles.map((tile) => tile.id)} strategy={rectSortingStrategy}>
          <div
            ref={grid}
            className={cn("grid gap-3 md:gap-4", className)}
            style={{ gridTemplateColumns: `repeat(${desktop ? cols : 1}, minmax(0, 1fr))` }}
          >
            {layout.tiles.map((tile) => (
              <EditableTile
                key={tile.id}
                tile={tile}
                name={names[tile.id]}
                cols={cols}
                desktop={desktop}
                editing={active}
                grid={grid.current}
                onHide={() => layout.updateTiles((tiles) => hideTile(tiles, tile.id))}
                onResize={(span) =>
                  layout.updateTiles((tiles) => resizeTile(tiles, tile.id, span, cols))
                }
              >
                {children(tile)}
              </EditableTile>
            ))}
          </div>
        </SortableContext>
      </DndContext>
    </>
  );
}

function EditableTile({
  tile,
  name,
  cols,
  desktop,
  editing,
  grid,
  onHide,
  onResize,
  children,
}: {
  tile: Tile;
  name: string;
  cols: Columns;
  desktop: boolean;
  editing: boolean;
  grid: HTMLDivElement | null;
  onHide: () => void;
  onResize: (span: number) => void;
  children: ReactNode;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: tile.id, disabled: !editing });
  const resizing = useRef<{ id: number; x: number; span: number; step: number } | null>(null);
  const spanLabel = t`${tile.span} of ${cols} columns`;
  return (
    <div
      ref={setNodeRef}
      data-panel={tile.id}
      data-span={tile.span}
      style={{
        gridColumn: `span ${desktop ? tile.span : 1} / span ${desktop ? tile.span : 1}`,
        transform: transform ? `translate3d(${transform.x}px, ${transform.y}px, 0)` : undefined,
        transition,
      }}
      className={cn(
        "relative flex min-w-0 flex-col [&>div>section]:h-full",
        editing && "rounded-2xl border border-dashed border-primary/60 bg-card shadow-tile",
        isDragging && "z-20 opacity-70",
      )}
    >
      {editing && (
        <div className="flex min-h-12 flex-wrap items-center gap-1 border-b border-divider px-2 py-1.5">
          <button
            type="button"
            ref={setActivatorNodeRef}
            {...attributes}
            {...listeners}
            aria-label={t`Move ${name}`}
            className="flex size-8 shrink-0 cursor-grab touch-none items-center justify-center rounded-lg text-muted-foreground hover:bg-muted active:cursor-grabbing"
          >
            <GripVertical className="size-4" aria-hidden />
          </button>
          <h2 className="mr-auto text-xs font-semibold">{name}</h2>
          <Button
            variant="ghost"
            size="icon-sm"
            disabled={tile.span <= minSpan(tile.id)}
            aria-label={t`Make ${name} narrower`}
            onClick={() => onResize(tile.span - 1)}
          >
            <Minus />
          </Button>
          <span
            className="font-display text-[11px] text-muted-foreground tabular-nums"
            aria-live="polite"
          >
            {spanLabel}
          </span>
          <Button
            variant="ghost"
            size="icon-sm"
            disabled={tile.span >= cols}
            aria-label={t`Make ${name} wider`}
            onClick={() => onResize(tile.span + 1)}
          >
            <Plus />
          </Button>
          <Button variant="ghost" size="icon-sm" aria-label={t`Hide ${name}`} onClick={onHide}>
            <X />
          </Button>
        </div>
      )}
      <div
        inert={editing}
        aria-hidden={editing || undefined}
        className={cn(
          "min-h-0 flex-1",
          editing &&
            "pointer-events-none select-none opacity-40 [&>section]:border-0 [&>section]:shadow-none",
        )}
      >
        <TileEditing.Provider value={editing}>{children}</TileEditing.Provider>
      </div>
      {editing && (
        <button
          type="button"
          role="slider"
          aria-label={t`Resize ${name}`}
          aria-valuemin={minSpan(tile.id)}
          aria-valuemax={cols}
          aria-valuenow={tile.span}
          aria-valuetext={spanLabel}
          aria-orientation="horizontal"
          className="absolute top-12 right-0 bottom-0 flex w-3 touch-none cursor-col-resize items-center justify-center rounded-br-2xl hover:bg-primary/15 focus-visible:bg-primary/15 focus-visible:outline-2 focus-visible:outline-primary"
          onKeyDown={(event) => {
            const span =
              event.key === "ArrowLeft"
                ? tile.span - 1
                : event.key === "ArrowRight"
                  ? tile.span + 1
                  : event.key === "Home"
                    ? minSpan(tile.id)
                    : event.key === "End"
                      ? cols
                      : null;
            if (span !== null) {
              event.preventDefault();
              onResize(span);
            }
          }}
          onPointerDown={(event) => {
            if (!grid || event.button !== 0) return;
            const gap = Number.parseFloat(getComputedStyle(grid).columnGap) || 0;
            resizing.current = {
              id: event.pointerId,
              x: event.clientX,
              span: tile.span,
              step: (grid.getBoundingClientRect().width + gap) / cols,
            };
            event.currentTarget.setPointerCapture(event.pointerId);
            event.preventDefault();
          }}
          onPointerMove={(event) => {
            const start = resizing.current;
            if (start && start.id === event.pointerId && start.step > 0)
              onResize(start.span + Math.round((event.clientX - start.x) / start.step));
          }}
          onPointerUp={() => {
            resizing.current = null;
          }}
          onLostPointerCapture={() => {
            resizing.current = null;
          }}
          onPointerCancel={() => {
            if (resizing.current) onResize(resizing.current.span);
            resizing.current = null;
          }}
        >
          <span className="h-8 w-1 rounded bg-primary/40" aria-hidden />
        </button>
      )}
    </div>
  );
}
