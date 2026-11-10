import { describe, expect, it } from "vitest";
import type { OperatorItem } from "./model";
import { fillPins, parsePins } from "./pins";

const row = (id: string, attention = false): OperatorItem => ({
  switcher_id: id,
  switcher_name: id,
  workspace_id: "w",
  workspace_name: "Studio",
  workspace_slug: "studio",
  connected: true,
  can_control: true,
  obs_connected: true,
  streaming: true,
  recording: false,
  paused: false,
  state: "live",
  attention: attention ? "low" : null,
});
describe("operator pins", () => {
  it("loads only valid unique pins and caps the saved layout at four", () => {
    const pins = parsePins(
      JSON.stringify([
        { workspace: "studio", switcher: "1" },
        { workspace: "studio", switcher: "1" },
        { workspace: 3, switcher: "bad" },
        ...["2", "3", "4", "5"].map((switcher) => ({ workspace: "other", switcher })),
      ]),
    );
    expect(pins.map((pin) => pin.switcher)).toEqual(["1", "2", "3", "4"]);
  });
  it("handles empty, unavailable and corrupt saved layouts", () => {
    for (const value of [null, "", "{", "{}", '[null,2,"x",{"workspace":"","switcher":"a"}]'])
      expect(parsePins(value)).toEqual([]);
  });
  it("fills attention first, preserves existing pins, skips offline and avoids duplicates", () => {
    const pins = [{ workspace: "elsewhere", switcher: "saved" }];
    const items = [
      row("normal"),
      row("low", true),
      { ...row("offline"), connected: false },
      row("saved"),
      row("last"),
      row("extra"),
    ];
    expect(fillPins(pins, items).map((pin) => pin.switcher)).toEqual([
      "saved",
      "low",
      "extra",
      "last",
    ]);
    expect(fillPins(pins, [])).toEqual(pins);
    expect(pins).toHaveLength(1);
  });
});
