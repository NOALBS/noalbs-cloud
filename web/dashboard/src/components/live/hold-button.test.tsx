import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HOLD_MS, HoldButton } from "./hold-button";

let host: HTMLDivElement;
let root: Root;
const complete = vi.fn();
const button = () => host.querySelector("button") as HTMLButtonElement;
const pointer = (type: string) =>
  act(() => {
    button().dispatchEvent(
      new MouseEvent(type === "pointerleave" ? "pointerout" : type, {
        bubbles: true,
        button: 0,
        relatedTarget: document.body,
      }),
    );
  });
const key = (type: string, key: string, repeat = false) =>
  act(() => {
    button().dispatchEvent(new KeyboardEvent(type, { key, repeat, bubbles: true }));
  });
const advance = (ms: number) => act(() => vi.advanceTimersByTime(ms));
const render = (disabled = false) =>
  act(() =>
    root.render(
      <HoldButton disabled={disabled} onComplete={complete}>
        Stop
      </HoldButton>,
    ),
  );

describe("hold to stop", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    complete.mockClear();
    (
      globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    render();
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    vi.useRealTimers();
  });
  it("does not fire on a short click; fills and fires once on a full hold", () => {
    pointer("pointerdown");
    advance(300);
    pointer("pointerup");
    advance(HOLD_MS);
    expect(complete).not.toHaveBeenCalled();
    pointer("pointerdown");
    expect(button().dataset.holding).toBe("true");
    advance(HOLD_MS);
    expect(complete).toHaveBeenCalledTimes(1);
    advance(HOLD_MS);
    expect(complete).toHaveBeenCalledTimes(1);
  });
  it.each(["Enter", " "])("supports holding %s without repeat activation", (input) => {
    key("keydown", input);
    advance(500);
    key("keydown", input, true);
    advance(500);
    expect(complete).toHaveBeenCalledTimes(1);
    key("keyup", input);
    advance(HOLD_MS);
    expect(complete).toHaveBeenCalledTimes(1);
  });
  it.each(["pointercancel", "pointerleave", "lostpointercapture"])("cancels on %s", (input) => {
    pointer("pointerdown");
    advance(600);
    pointer(input);
    advance(HOLD_MS);
    expect(complete).not.toHaveBeenCalled();
  });
  it("cancels when disabled or window loses focus", () => {
    pointer("pointerdown");
    advance(600);
    render(true);
    advance(HOLD_MS);
    expect(complete).not.toHaveBeenCalled();
    render();
    pointer("pointerdown");
    act(() => window.dispatchEvent(new Event("blur")));
    advance(HOLD_MS);
    expect(complete).not.toHaveBeenCalled();
  });
  it("clears an in-progress hold when unmounted", () => {
    pointer("pointerdown");
    act(() => root.render(null));
    advance(HOLD_MS);
    expect(complete).not.toHaveBeenCalled();
  });
});
