import { describe, expect, it } from "vitest";
import { canControlConnect, connectAccessOptions, watchdogOptions } from "./connect";

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
});
