import { describe, expect, it } from "vitest";
import { legacySwitcherPage } from "./legacy-switcher-page";

describe("legacySwitcherPage", () => {
  it.each([
    ["scenes", "scenes"],
    ["servers", "servers"],
    ["triggers", "rules"],
  ] as const)("maps %s", (segment, hash) => {
    expect(legacySwitcherPage(segment, "ignored")).toEqual({
      to: "/$workspace/s/$switcher/switching",
      hash,
    });
  });
  it.each(["belabox", "moblin", "connect"])("maps device %s", (hash) => {
    expect(legacySwitcherPage("obs", `#${hash}`)).toEqual({
      to: "/$workspace/s/$switcher/devices",
      hash,
    });
  });
  it.each(["", "preview", "unknown"])("preserves settings %s", (hash) => {
    expect(legacySwitcherPage("obs", hash)).toEqual({
      to: "/$workspace/s/$switcher/settings",
      hash,
    });
  });
});
