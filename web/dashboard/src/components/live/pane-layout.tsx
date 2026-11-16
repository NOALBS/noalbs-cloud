import {
  DndContext,
  DragOverlay,
  PointerSensor,
  pointerWithin,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import { ArrowDown, ArrowUp, GripVertical, LayoutDashboard, Plus, X } from "lucide-react";
import {
  type CSSProperties,
  Fragment,
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useMediaQuery } from "@/hooks/use-media-query";
import {
  type Action,
  type Address,
  defaults,
  type Mode,
  type PaneId,
  paneDefinitions,
  type Row,
  read,
  reduceRows,
  type Saved,
  slotWidths,
  storageKey,
  type Target,
  title,
  type VisualRow,
  wrapRows,
} from "./pane-layout-model";
import { RowDivider, SlotDivider } from "./pane-layout-resize";
import "./live.css";

export type { PaneId } from "./pane-layout-model";
export { paneDefinitions, storageKey } from "./pane-layout-model";

const targetId = (target: Target) => JSON.stringify(target);
function DropZone({
  target,
  className,
  label,
}: {
  target: Target;
  className: string;
  label: string;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: targetId(target), data: { target } });
  return (
    <div
      ref={setNodeRef}
      className={className}
      data-drop-active={isOver}
      data-drop-kind={target.kind}
      data-drop-row={target.row}
      data-drop-slot={"slot" in target ? target.slot : undefined}
    >
      <span>{label}</span>
    </div>
  );
}
function PaneMenu({ available, add }: { available: PaneId[]; add: (id: PaneId) => void }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" size="sm" disabled={!available.length}>
          <Plus />
          Add pane
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent>
        {available.map((id) => (
          <DropdownMenuItem key={id} onSelect={() => add(id)}>
            {title(id)}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
function AddRow({
  at,
  edit,
  dragging,
}: {
  at: number;
  edit: (a: Action) => void;
  dragging: boolean;
}) {
  return (
    <div className="live-add-row" data-add-row={at}>
      {dragging ? (
        <DropZone
          target={{ kind: "row", row: at }}
          className="live-row-drop"
          label="Move to a new row"
        />
      ) : (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="sm">
              <Plus />
              Add row
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent>
            {[1, 2, 3, 4].map((n) => (
              <DropdownMenuItem key={n} onSelect={() => edit({ type: "add-row", at, columns: n })}>
                {n} {n === 1 ? "column" : "columns"}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      )}
    </div>
  );
}
function PaneSlot({
  id,
  address,
  editing,
  full,
  dragging,
  children,
  edit,
}: {
  id: PaneId;
  address: Address;
  editing: boolean;
  full: boolean;
  dragging: boolean;
  children: ReactNode;
  edit: (a: Action) => void;
}) {
  const drag = useDraggable({ id, data: address, disabled: !editing });
  return (
    <div ref={drag.setNodeRef} data-pane={id} className="live-pane" data-dragging={drag.isDragging}>
      {editing && (
        <div className="live-pane-edit-bar">
          <button
            type="button"
            className="live-pane-drag"
            aria-label={`Drag ${title(id)}`}
            {...drag.listeners}
            {...drag.attributes}
          >
            <GripVertical />
            {title(id)}
          </button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={`Add beside ${title(id)}`}
            disabled={full}
            onClick={() => edit({ type: "beside", ...address })}
          >
            <Plus />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={`Hide ${title(id)}`}
            onClick={() => edit({ type: "hide", ...address })}
          >
            <X />
          </Button>
        </div>
      )}
      <div className="live-pane-fit" inert={editing || undefined}>
        {children}
      </div>
      {editing && dragging && (
        <>
          <DropZone
            target={{ kind: "center", ...address }}
            className="live-pane-drop"
            label="Swap panes"
          />
          {!full && (
            <>
              <DropZone
                target={{ kind: "left", ...address }}
                className="live-edge-drop live-edge-left"
                label="Insert before"
              />
              <DropZone
                target={{ kind: "right", ...address }}
                className="live-edge-drop live-edge-right"
                label="Insert after"
              />
            </>
          )}
        </>
      )}
    </div>
  );
}
function DesktopRow({
  visual,
  row,
  width,
  editing,
  dragging,
  panes,
  available,
  edit,
  first,
}: {
  visual: VisualRow;
  row: Row;
  width: number;
  editing: boolean;
  dragging: boolean;
  panes: Record<PaneId, ReactNode>;
  available: PaneId[];
  edit: (a: Action) => void;
  first: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const widths = slotWidths(
    visual.slots.map((s) => s.slot),
    width,
  );
  return (
    <div className="live-row-wrap" data-source-row={visual.source}>
      {editing && first && (
        <div className="live-row-tools">
          <span>
            Row {visual.source + 1} · {row.slots.length}{" "}
            {row.slots.length === 1 ? "column" : "columns"}
          </span>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={`Remove row ${visual.source + 1}`}
            onClick={() => edit({ type: "remove-row", row: visual.source })}
          >
            <X />
          </Button>
        </div>
      )}
      <div
        ref={ref}
        className="live-row"
        data-visual-row
        style={{ "--row-min-height": `${visual.minHeight ?? 0}px` } as CSSProperties}
      >
        {visual.slots.map((s, i) => (
          <Fragment key={s.index}>
            {editing && i > 0 && (
              <SlotDivider
                container={ref}
                row={row}
                source={visual.source}
                left={visual.slots[i - 1] ?? s}
                right={s}
                edit={edit}
              />
            )}
            <div
              className="live-slot"
              data-slot={s.index}
              style={{ "--slot-width": `${widths[i]}px` } as CSSProperties}
            >
              {s.slot.pane ? (
                <PaneSlot
                  id={s.slot.pane}
                  address={{ row: visual.source, slot: s.index }}
                  editing={editing}
                  full={row.slots.length >= 4}
                  dragging={dragging}
                  edit={edit}
                >
                  {panes[s.slot.pane]}
                </PaneSlot>
              ) : (
                <div className="live-empty-slot">
                  <span>Empty slot</span>
                  <PaneMenu
                    available={available}
                    add={(pane) => edit({ type: "place", row: visual.source, slot: s.index, pane })}
                  />
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    className="live-remove-slot"
                    aria-label="Remove empty slot"
                    onClick={() => edit({ type: "remove-slot", row: visual.source, slot: s.index })}
                  >
                    <X />
                  </Button>
                  {dragging && (
                    <DropZone
                      target={{ kind: "center", row: visual.source, slot: s.index }}
                      className="live-pane-drop"
                      label="Place here"
                    />
                  )}
                </div>
              )}
            </div>
          </Fragment>
        ))}
      </div>
      {editing && (
        <RowDivider container={ref} source={visual.source} minHeight={row.minHeight} edit={edit} />
      )}
    </div>
  );
}
export function PaneLayout({
  switcherId,
  mode,
  panes,
  toolbar,
}: {
  switcherId: string;
  mode: Mode;
  panes: Record<PaneId, ReactNode>;
  toolbar: HTMLElement | null;
}) {
  const phone = !useMediaQuery("(min-width: 768px)");
  const key = storageKey(switcherId, mode);
  const [state, setState] = useState<{
    key: string;
    saved: Saved;
    draft: Saved | null;
    undo: Saved[];
  }>(() => ({ key, saved: read(key, mode), draft: null, undo: [] }));
  if (state.key !== key) setState({ key, saved: read(key, mode), draft: null, undo: [] });
  const [phoneEditing, setPhoneEditing] = useState(false);
  const [active, setActive] = useState<PaneId | null>(null);
  const editing = phone ? phoneEditing : Boolean(state.draft);
  const current = state.draft ?? state.saved;
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 6 } }));
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setWidth(el.clientWidth));
    observer.observe(el);
    setWidth(el.clientWidth);
    return () => observer.disconnect();
  }, []);
  const cancel = useCallback(() => {
    setActive(null);
    setState((s) => ({ ...s, draft: null, undo: [] }));
  }, []);
  useEffect(() => {
    if (!editing) return;
    const cancelOnEscape = (e: KeyboardEvent) => {
      if (
        e.key === "Escape" &&
        !e.defaultPrevented &&
        !document.querySelector('[role="menu"], [role="dialog"], [role="alertdialog"]')
      ) {
        e.preventDefault();
        cancel();
        setPhoneEditing(false);
      }
    };
    window.addEventListener("keydown", cancelOnEscape, true);
    return () => window.removeEventListener("keydown", cancelOnEscape, true);
  }, [editing, cancel]);
  const persist = (value: Saved) => {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {
      /* The choice still lasts this visit. */
    }
    setState((s) => ({ ...s, saved: value, draft: null, undo: [] }));
  };
  const update = (value: Saved) =>
    setState((s) => (s.draft ? { ...s, draft: value, undo: [...s.undo, s.draft] } : s));
  const edit = (action: Action) => {
    const rows = reduceRows(current.desktop, action);
    if (rows !== current.desktop) update({ ...current, desktop: rows });
  };
  const offered = (id: PaneId) =>
    panes[id] != null &&
    (mode === "operator" || !paneDefinitions.some((p) => p.id === id && "operator" in p));
  const placed = phone
    ? current.phone
    : current.desktop.flatMap((r) => r.slots.flatMap((s) => (s.pane ? [s.pane] : [])));
  const available = paneDefinitions
    .map((p) => p.id)
    .filter((id) => offered(id) && !placed.includes(id));
  const visual = wrapRows(current.desktop, width, editing, offered);
  const tools = phone ? (
    <>
      {phoneEditing && (
        <>
          <PaneMenu
            available={available}
            add={(id) => persist({ ...state.saved, phone: [...state.saved.phone, id] })}
          />
          <Button
            variant="outline"
            size="sm"
            onClick={() => persist({ ...state.saved, phone: defaults(mode).phone })}
          >
            Reset
          </Button>
        </>
      )}
      <Button
        variant={phoneEditing ? "default" : "outline"}
        size="sm"
        onClick={() => setPhoneEditing((v) => !v)}
      >
        {!phoneEditing && <LayoutDashboard />}
        {phoneEditing ? "Done" : "Edit layout"}
      </Button>
    </>
  ) : state.draft ? (
    <>
      <Button
        variant="outline"
        size="sm"
        disabled={!state.undo.length}
        onClick={() =>
          setState((s) => ({ ...s, draft: s.undo.at(-1) ?? s.draft, undo: s.undo.slice(0, -1) }))
        }
      >
        Undo
      </Button>
      <Button
        variant="outline"
        size="sm"
        onClick={() => update({ ...current, desktop: defaults(mode).desktop })}
      >
        Reset to default
      </Button>
      <Button variant="outline" size="sm" onClick={cancel}>
        Cancel
      </Button>
      <Button size="sm" onClick={() => persist(current)}>
        Save
      </Button>
    </>
  ) : (
    <Button
      variant="outline"
      size="sm"
      onClick={() => setState((s) => ({ ...s, draft: s.saved, undo: [] }))}
    >
      <LayoutDashboard />
      Edit layout
    </Button>
  );
  return (
    <div ref={ref} data-testid="live-pane-layout" data-editing={editing}>
      {toolbar ? (
        createPortal(tools, toolbar)
      ) : (
        <div className="mb-3 flex justify-end gap-2">{tools}</div>
      )}
      {phone ? (
        <div className="flex flex-col gap-3">
          {current.phone.filter(offered).map((id) => (
            <div key={id} className="live-pane" data-pane={id}>
              <div className="live-pane-fit" inert={phoneEditing || undefined}>
                {panes[id]}
              </div>
              {phoneEditing && (
                <div className="live-phone-overlay">
                  <div className="live-phone-edit">
                    <span>{title(id)}</span>
                    {[-1, 1].map((by) => {
                      const index = current.phone.indexOf(id);
                      return (
                        <Button
                          key={by}
                          size="icon-sm"
                          variant="ghost"
                          aria-label={`Move ${title(id)} ${by < 0 ? "up" : "down"}`}
                          disabled={index + by < 0 || index + by >= current.phone.length}
                          onClick={() => {
                            const order = [...current.phone];
                            const other = order[index + by];
                            if (!other) return;
                            order[index] = other;
                            order[index + by] = id;
                            persist({ ...state.saved, phone: order });
                          }}
                        >
                          {by < 0 ? <ArrowUp /> : <ArrowDown />}
                        </Button>
                      );
                    })}
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      aria-label={`Hide ${title(id)}`}
                      onClick={() =>
                        persist({ ...state.saved, phone: current.phone.filter((p) => p !== id) })
                      }
                    >
                      <X />
                    </Button>
                  </div>
                </div>
              )}
            </div>
          ))}
        </div>
      ) : (
        <DndContext
          sensors={sensors}
          collisionDetection={(args) => {
            const hits = pointerWithin(args);
            return [...hits].sort(
              (a, b) =>
                (String(b.id).includes('"left"') || String(b.id).includes('"right"') ? 1 : 0) -
                (String(a.id).includes('"left"') || String(a.id).includes('"right"') ? 1 : 0),
            );
          }}
          onDragStart={(e) => setActive(e.active.id as PaneId)}
          onDragCancel={() => setActive(null)}
          onDragEnd={(e) => {
            if (e.over?.data.current?.target && e.active.data.current)
              edit({
                type: "drop",
                source: e.active.data.current as Address,
                target: e.over.data.current.target as Target,
              });
            setActive(null);
          }}
        >
          <div className="live-rows">
            {current.desktop.map((row, index) => {
              const parts = visual.filter((v) => v.source === index);
              return (
                // biome-ignore lint/suspicious/noArrayIndexKey: Rows use positional addresses; changing weights must preserve focused handles.
                <Fragment key={index}>
                  {editing && <AddRow at={index} edit={edit} dragging={Boolean(active)} />}
                  {parts.map((v, i) => (
                    <DesktopRow
                      key={v.slots[0]?.index}
                      row={row}
                      visual={v}
                      width={width}
                      editing={editing}
                      dragging={Boolean(active)}
                      panes={panes}
                      available={available}
                      edit={edit}
                      first={i === 0}
                    />
                  ))}
                </Fragment>
              );
            })}
            {editing && (
              <AddRow at={current.desktop.length} edit={edit} dragging={Boolean(active)} />
            )}
          </div>
          {createPortal(
            <DragOverlay dropAnimation={null}>
              {active && (
                <div className="live-drag-ghost">
                  <GripVertical />
                  {title(active)}
                </div>
              )}
            </DragOverlay>,
            document.body,
          )}
        </DndContext>
      )}
    </div>
  );
}
