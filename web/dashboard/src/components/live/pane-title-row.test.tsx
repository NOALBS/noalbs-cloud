import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PaneEditingContext, PaneTitleRow } from "./pane-title-row";

let host: HTMLDivElement;
let root: Root;
const pointer = vi.fn();
const beside = vi.fn();
const hide = vi.fn();
const activator = vi.fn();
const attributes = {
  role: "button",
  tabIndex: 0,
  "aria-disabled": false,
  "aria-pressed": undefined,
  "aria-roledescription": "draggable",
  "aria-describedby": "drag-help",
};
const render = (editing: boolean, full = false) =>
  act(() =>
    root.render(
      <PaneEditingContext.Provider
        value={{
          editing,
          title: "Preview",
          full,
          beside,
          hide,
          drag: {
            attributes,
            listeners: { onPointerDown: pointer },
            setActivatorNodeRef: activator,
          },
        }}
      >
        <div className="live-pane-fit">
          <section>
            <PaneTitleRow>
              <h2>Preview</h2>
            </PaneTitleRow>
            <div data-body>
              <button type="button">Pane action</button>
            </div>
          </section>
        </div>
      </PaneEditingContext.Provider>,
    ),
  );

describe("pane layout title row", () => {
  beforeEach(() => {
    (
      globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    vi.clearAllMocks();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
  });
  it("keeps one title and the same header node when editing starts and stops", () => {
    render(false);
    const header = host.querySelector("[data-pane-title-row]");
    expect(host.querySelectorAll("h2")).toHaveLength(1);
    expect(host.querySelector('[aria-label="Hide Preview"]')).toBeNull();
    render(true);
    expect(host.querySelector("[data-pane-title-row]")).toBe(header);
    expect(host.querySelectorAll("h2")).toHaveLength(1);
    expect(header?.getAttribute("role")).toBe("group");
    expect(activator).toHaveBeenCalledWith(host.querySelector('[aria-label="Drag Preview"]'));
    expect(host.querySelector<HTMLElement>("[data-body]")?.inert).toBe(true);
    render(false);
    expect(host.querySelector("[data-pane-title-row]")).toBe(header);
    expect(Boolean(host.querySelector<HTMLElement>("[data-body]")?.inert)).toBe(false);
    expect(header?.getAttribute("role")).toBeNull();
  });
  it("uses the entire title row for dragging without dragging from its controls", () => {
    render(true);
    act(() =>
      host
        .querySelector("h2")
        ?.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 })),
    );
    expect(pointer).toHaveBeenCalledOnce();
    const button = host.querySelector<HTMLButtonElement>('[aria-label="Add beside Preview"]');
    act(() => {
      button?.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
      button?.click();
    });
    expect(pointer).toHaveBeenCalledOnce();
    expect(beside).toHaveBeenCalledOnce();
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Hide Preview"]')?.click());
    expect(hide).toHaveBeenCalledOnce();
  });
  it("disables Add beside for a full row", () => {
    render(true, true);
    expect(
      host.querySelector<HTMLButtonElement>('[aria-label="Add beside Preview"]')?.disabled,
    ).toBe(true);
  });
  it("is noninteractive outside a layout and can omit layout-only headings", () => {
    act(() =>
      root.render(
        <>
          <PaneTitleRow>
            <h2>Preview</h2>
          </PaneTitleRow>
          <PaneTitleRow layoutOnly>
            <h2>Actions</h2>
          </PaneTitleRow>
        </>,
      ),
    );
    expect(host.querySelectorAll("h2")).toHaveLength(1);
    expect(host.querySelector("[role=button]")).toBeNull();
    expect(host.querySelector("[data-layout-controls]")).toBeNull();
  });
});
