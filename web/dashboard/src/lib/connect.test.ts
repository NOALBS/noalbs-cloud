import { describe, expect, it } from "vitest";
import type { PcObsState } from "@/api/types";
import {
  canControlConnect,
  canManageConnectPrograms,
  canViewConnectLogs,
  connectAccessOptions,
  connectObsActionAvailability,
  pcObsLabel,
  watchdogOptions,
} from "./connect";

describe("Connect settings", () => {
  it("names roles and organization managers", () => {
    expect(connectAccessOptions(null).map((option) => option.label)).toEqual([
      "Only the owner",
      "The owner and admins",
      "Everyone in the workspace",
    ]);
    expect(connectAccessOptions("Acme IRL")[0]?.label).toBe("Only Acme IRL");
    expect(connectAccessOptions("Acme IRL")[1]?.label).toBe("Acme IRL and admins");
    expect(watchdogOptions().map((option) => option.value)).toEqual([
      "off",
      "restart",
      "restart_resume",
    ]);
    expect(watchdogOptions().every((option) => option.description.trim().length > 0)).toBe(true);
  });

  it("labels every Connect OBS state", () => {
    const states: PcObsState[] = [
      "unknown",
      "not_running",
      "starting",
      "connected",
      "not_responding",
      "waiting_for_input",
      "safe_mode",
      "crashed",
      "websocket_disabled",
    ];
    expect(states.map(pcObsLabel)).toEqual([
      "OBS state unknown",
      "OBS not running",
      "OBS starting",
      "OBS connected",
      "OBS not responding",
      "OBS is waiting for input",
      "OBS is in safe mode",
      "OBS crashed",
      "OBS WebSocket is disabled",
    ]);
  });

  it("allows watchdog changes to the configured roles but never support", () => {
    const viewers = [
      { role: "owner", staff: false },
      { role: "admin", staff: false },
      { role: "operator", staff: false },
      { role: null, staff: true, support: "control" },
    ] as const;
    expect(
      ["owners", "admins", "everyone"].map((level) =>
        viewers.map((actor) => canControlConnect(level as "owners" | "admins" | "everyone", actor)),
      ),
    ).toEqual([
      [true, false, false, false],
      [true, true, false, false],
      [true, true, true, false],
    ]);
  });

  it("shows Connect logs only to roles allowed by the log setting", () => {
    const owner = { role: "owner", staff: false } as const;
    const member = { role: "operator", staff: false } as const;
    expect(canViewConnectLogs("owners", owner)).toBe(true);
    expect(canViewConnectLogs("owners", member)).toBe(false);
    expect(canViewConnectLogs("everyone", member)).toBe(true);
  });

  it("shows program management only to roles allowed by its setting", () => {
    const owner = { role: "owner", staff: false } as const;
    const member = { role: "operator", staff: false } as const;
    const support = { role: null, staff: true, support: "control" } as const;
    expect(canManageConnectPrograms("owners", owner)).toBe(true);
    expect(canManageConnectPrograms("owners", member)).toBe(false);
    expect(canManageConnectPrograms("everyone", member)).toBe(true);
    expect(canManageConnectPrograms("everyone", support)).toBe(false);
  });

  it("enables OBS actions only for the states that support them", () => {
    expect(connectObsActionAvailability(undefined)).toEqual({
      visible: false,
      canStart: false,
      canStopOrRestart: false,
    });
    expect(connectObsActionAvailability("not_running")).toEqual({
      visible: true,
      canStart: true,
      canStopOrRestart: false,
    });
    expect(connectObsActionAvailability("crashed")).toEqual({
      visible: true,
      canStart: true,
      canStopOrRestart: true,
    });
    expect(connectObsActionAvailability("unknown")).toEqual({
      visible: true,
      canStart: false,
      canStopOrRestart: false,
    });
  });
});
