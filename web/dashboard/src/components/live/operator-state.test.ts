import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  initialOperator,
  operatorHotkey,
  operatorKey,
  readOperator,
  swapAfterTake,
  switchedBy,
} from "./operator-state";

const context = { desktop: true, blocked: false, editing: false, button: false };
const key = (key: string, more = {}) => ({
  key,
  code: "",
  shiftKey: false,
  altKey: false,
  ctrlKey: false,
  metaKey: false,
  repeat: false,
  isComposing: false,
  ...more,
});

describe("operator shortcuts", () => {
  it("maps numbers, shifted physical digits, Take, Cut, help and immersive", () => {
    expect(operatorHotkey(key("3"), context)).toEqual({ action: "scene", index: 2, direct: false });
    expect(operatorHotkey(key("!", { code: "Digit1", shiftKey: true }), context)).toEqual({
      action: "scene",
      index: 0,
      direct: true,
    });
    expect(operatorHotkey(key(" "), context)).toEqual({ action: "take" });
    expect(operatorHotkey(key("Enter"), context)).toEqual({ action: "cut" });
    expect(operatorHotkey(key("?", { shiftKey: true }), context)).toEqual({ action: "help" });
    expect(operatorHotkey(key("f"), context)).toEqual({ action: "immersive" });
  });
  it("ignores fields/dialogs, Customize mode and phones", () => {
    for (const change of [{ blocked: true }, { editing: true }, { desktop: false }])
      for (const input of ["1", " ", "Enter", "?", "f"])
        expect(operatorHotkey(key(input), { ...context, ...change })).toBeNull();
  });
  it("ignores repeats, composition and browser modifiers", () => {
    for (const change of [
      { repeat: true },
      { isComposing: true },
      { ctrlKey: true },
      { altKey: true },
      { metaKey: true },
    ])
      expect(operatorHotkey(key("1", change), context)).toBeNull();
  });
  it("leaves focused buttons' Enter/Space to the button and has no stream/record shortcut", () => {
    for (const input of [" ", "Enter"])
      expect(operatorHotkey(key(input), { ...context, button: true })).toBeNull();
    for (const input of ["r", "s", "0", "Escape", "m"])
      expect(operatorHotkey(key(input), context)).toBeNull();
  });
});

describe("local studio preference and swaps", () => {
  beforeEach(() => localStorage.clear());
  it("loads safe defaults for missing, corrupt and denied storage", () => {
    expect(readOperator("one")).toEqual(initialOperator);
    localStorage.setItem(operatorKey("one"), "{");
    expect(readOperator("one")).toEqual(initialOperator);
    const spy = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    expect(readOperator("one")).toEqual(initialOperator);
    spy.mockRestore();
  });
  it("isolates switchers and validates transition, duration and cue", () => {
    localStorage.setItem(
      operatorKey("one"),
      JSON.stringify({
        studio: false,
        preview: { uuid: "two", name: "B" },
        transition: "Stinger",
        duration: 1200,
      }),
    );
    expect(readOperator("one")).toMatchObject({
      studio: false,
      transition: "Stinger",
      duration: 1200,
    });
    expect(readOperator("two")).toEqual(initialOperator);
    localStorage.setItem(
      operatorKey("one"),
      JSON.stringify({ transition: "bogus", duration: -20, preview: "x" }),
    );
    expect(readOperator("one")).toMatchObject({ transition: "Fade", duration: 0, preview: null });
  });
  it("puts the old Program in Preview only after a successful take", () => {
    const state = { ...initialOperator, preview: { uuid: "b", name: "B" } };
    const previous = { uuid: "a", name: "A" };
    expect(swapAfterTake(state, previous).preview).toEqual(previous);
    expect(state.preview?.uuid).toBe("b");
    expect(swapAfterTake(state, null).preview).toBeNull();
  });
  it("shows only known attribution for the latest matching scene change", () => {
    const event = {
      id: "1",
      created_at: "2026-01-01T00:00:00Z",
      kind: "scene_switched" as const,
      data: { to: "A", reason: "obs" as const },
    };
    expect(switchedBy([], "A")).toBeNull();
    expect(switchedBy([event], "B")).toBeNull();
    expect(switchedBy([event], "A")).toBe("Changed in OBS");
    expect(
      switchedBy([{ ...event, data: { ...event.data, reason: "auto", bitrate: 612 } }], "A"),
    ).toBe("auto switch · 612 kbps");
  });
});
