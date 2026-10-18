import { describe, expect, it } from "vitest";
import type { MoblinStatus } from "@/api/types";
import {
  batteryLabel,
  bondingRows,
  canTurnOffConnection,
  moblinkRelays,
  moblinView,
  sortedPresets,
  stepZoom,
  systemMonitorLabel,
  thermalLabel,
  zoomLabel,
} from "./moblin";

const now = Date.parse("2026-09-29T12:00:00Z");

const status = (change: Partial<MoblinStatus> = {}): MoblinStatus => ({
  updated_at: "2026-09-29T11:59:58Z",
  paused: false,
  streaming: false,
  recording: false,
  muted: false,
  battery_level: null,
  battery_charging: null,
  thermal: null,
  bitrate: null,
  bonding: null,
  bonding_rtts: null,
  uptime: null,
  scene_id: null,
  scenes: [],
  bitrate_preset_id: null,
  bitrate_presets: [],
  torch: false,
  stealth_mode: false,
  system_monitor: null,
  moblink: null,
  dji_devices: null,
  connection_priorities_enabled: false,
  connections: [],
  zoom: null,
  zoom_presets: [],
  zoom_preset_id: null,
  ...change,
});

describe("moblinView", () => {
  it("treats a missing or old status as disconnected", () => {
    expect(moblinView(undefined, now)).toEqual({ kind: "loading" });
    expect(moblinView(null, now)).toEqual({ kind: "disconnected" });
    expect(moblinView(status({ updated_at: "2026-09-29T11:58:00Z" }), now)).toEqual({
      kind: "disconnected",
    });
    expect(moblinView(status(), now).kind).toBe("connected");
    expect(moblinView(status({ paused: true }), now).kind).toBe("paused");
  });
});

describe("labels", () => {
  it("names the heat Moblin reports", () => {
    expect(thermalLabel("normal")).toBe("Normal");
    expect(thermalLabel(null)).toBeNull();
    expect(thermalLabel("serious")).toBe("Getting hot");
    expect(thermalLabel("critical")).toBe("Hot");
  });

  it("says the battery level and whether it charges", () => {
    expect(batteryLabel(status())).toBeNull();
    expect(batteryLabel(status({ battery_level: 87 }))).toBe("87%");
    expect(batteryLabel(status({ battery_level: 40, battery_charging: true }))).toBe(
      "40% · charging",
    );
  });

  it("lists presets from low to high", () => {
    const presets = sortedPresets(
      status({
        bitrate_presets: [
          { id: "b", kbps: 6000 },
          { id: "a", kbps: 1000 },
        ],
      }),
    );
    expect(presets.map((preset) => preset.kbps)).toEqual([1000, 6000]);
  });
});

describe("system monitor, zoom and connections", () => {
  it("spells out Moblin's system monitor line", () => {
    expect(systemMonitorLabel("12%/45% 310 MB")).toBe("CPU 45% (Moblin 12%) · 310 MB");
    expect(systemMonitorLabel("something else")).toBe("something else");
  });

  it("zooms in and out by a quarter, within range", () => {
    expect(stepZoom(1, 1)).toBe(1.3);
    expect(stepZoom(1, -1)).toBe(0.8);
    expect(stepZoom(0.5, -1)).toBe(0.5);
    expect(stepZoom(99, 1)).toBe(100);
    expect(zoomLabel(2.25)).toBe("2.3x");
    expect(zoomLabel(1)).toBe("1x");
  });

  it("keeps one connection on", () => {
    const connections = [
      { id: "a", name: "Cellular", priority: 1, enabled: true },
      { id: "b", name: "WiFi", priority: 1, enabled: false },
    ];
    expect(canTurnOffConnection(connections, "a")).toBe(false);
    expect(canTurnOffConnection(connections, "b")).toBe(true);
  });
});

describe("bonding and Moblink lines", () => {
  it("merges share and RTT per connection", () => {
    expect(
      bondingRows("94% WiFi, 3% IP15, 3% IP15-50", "35 ms WiFi, 101 ms IP15, -1 ms IP15-50"),
    ).toEqual([
      { name: "WiFi", percent: 94, rttMs: 35 },
      { name: "IP15", percent: 3, rttMs: 101 },
      { name: "IP15-50", percent: 3, rttMs: null },
    ]);
    expect(bondingRows(null, null)).toBeNull();
    expect(bondingRows("something else", null)).toBeNull();
  });

  it("reads relays with their battery and heat", () => {
    expect(moblinkRelays("IP15🔋34%, IP15-50")).toEqual([
      { name: "IP15", battery: 34, lowBattery: false, hot: false },
      { name: "IP15-50", battery: null, lowBattery: false, hot: false },
    ]);
    expect(moblinkRelays("Old phone🔥🪫8%")).toEqual([
      { name: "Old phone", battery: 8, lowBattery: true, hot: true },
    ]);
    expect(moblinkRelays("Connected")).toBeNull();
  });
});
