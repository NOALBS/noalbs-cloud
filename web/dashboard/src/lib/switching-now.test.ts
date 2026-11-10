import { describe, expect, it } from "vitest";
import type { SceneRule, SwitcherStatus } from "@/api/types";
import { switchingNow } from "./switching-now";

const status: SwitcherStatus = {
  obs_connected: true,
  browser_source: true,
  auto_switch: true,
  streaming: true,
  recording: false,
  recording_paused: false,
  state: "live",
  current_scene: { uuid: "live", name: "IRL" },
  active_server_id: "a",
  updated_at: "2026-01-01T00:00:00Z",
};
const input = () => ({
  status,
  triggers: { low: 1000, offline: 300, rtt: 2000, rtt_offline: 5000 },
  behaviour: { only_switch_when_streaming: true, switch_to_live: true },
  scenes: {
    live: "live",
    low: "low",
    offline: "brb",
    starting: "starting",
    ending: null,
    privacy: null,
    refresh: null,
  },
  servers: [
    {
      id: "a",
      name: "Camera",
      enabled: true,
      overrides: { live: null as string | null, low: null, offline: null },
    },
    {
      id: "b",
      name: "Backup",
      enabled: true,
      overrides: { live: null as string | null, low: null, offline: null },
    },
  ],
  rules: [] as SceneRule[],
  samples: [{ server_id: "a", bitrate: 6000, rtt: 230, state: "live" as const }],
});
describe("switchingNow", () => {
  it("waits for missing status", () =>
    expect(switchingNow({ ...input(), status: undefined }).text).toMatch(/Waiting/));
  it("handles an offline switcher", () =>
    expect(switchingNow({ ...input(), status: null }).text).toMatch(/switcher is offline/));
  it("handles disconnected OBS before any feed explanation", () =>
    expect(switchingNow({ ...input(), status: { ...status, obs_connected: false } })).toEqual({
      text: "OBS is not connected.",
    }));
  it("handles a paused switcher", () =>
    expect(switchingNow({ ...input(), status: { ...status, paused: true } }).text).toMatch(
      /paused/,
    ));
  it("reports auto switch off", () =>
    expect(switchingNow({ ...input(), status: { ...status, auto_switch: false } }).text).toMatch(
      /Auto switch is off/,
    ));
  it("reports idle with auto switch off", () =>
    expect(
      switchingNow({ ...input(), status: { ...status, auto_switch: false, streaming: false } })
        .text,
    ).toMatch(/not streaming and auto switch is off/));
  it("explains waiting for the stream", () =>
    expect(switchingNow({ ...input(), status: { ...status, streaming: false } }).text).toMatch(
      /takes over when the stream starts/,
    ));
  it("does not claim off-air switching waits if that setting is off", () =>
    expect(
      switchingNow({
        ...input(),
        status: { ...status, streaming: false },
        behaviour: { only_switch_when_streaming: false, switch_to_live: true },
      }).text,
    ).toMatch(/auto switching is enabled/));
  it("explains a healthy active server", () =>
    expect(switchingNow(input())).toEqual({
      state: "live",
      text: "Camera is above the LOW triggers.",
    }));
  it("uses only a rule actually applied to the current scene", () => {
    const rule: SceneRule = {
      id: "rule",
      server_ids: ["a", "b"],
      enabled: true,
      priority: 0,
      scenes: { live: "both", low: null, offline: null },
    };
    const data = { ...input(), rules: [rule] };
    expect(switchingNow(data).text).not.toMatch(/combination/);
    expect(
      switchingNow({
        ...data,
        status: {
          ...status,
          active_scene_rule_id: "rule",
          current_scene: { uuid: "both", name: "Two cameras" },
        },
      }).text,
    ).toMatch(/combination Camera \+ Backup picks the scene/);
    expect(
      switchingNow({ ...data, status: { ...status, active_scene_rule_id: "rule" } }).text,
    ).not.toMatch(/combination/);
  });
  it("identifies the inclusive LOW bitrate threshold", () =>
    expect(
      switchingNow({
        ...input(),
        status: { ...status, state: "low" },
        samples: [{ server_id: "a", bitrate: 1000, state: "low" }],
      }).text,
    ).toMatch(/at or below.*1000 kbps/));
  it("identifies the inclusive LOW RTT threshold", () =>
    expect(
      switchingNow({
        ...input(),
        status: { ...status, state: "low" },
        samples: [{ server_id: "a", bitrate: 6000, rtt: 2000, state: "low" }],
      }).text,
    ).toMatch(/at or above.*2000 ms/));
  it("does not invent a LOW trigger when readings are absent", () =>
    expect(
      switchingNow({ ...input(), status: { ...status, state: "low" }, samples: [] }).text,
    ).toMatch(/no current reading/));
  it("does not claim healthy thresholds when readings contradict the state", () =>
    expect(
      switchingNow({ ...input(), samples: [{ server_id: "a", bitrate: 100, state: "live" }] }).text,
    ).toBe("NOALBS reports a LIVE feed."));
  it("explains BRB without claiming the scene already switched", () =>
    expect(
      switchingNow({ ...input(), status: { ...status, state: "offline", active_server_id: null } }),
    ).toEqual({ state: "offline", text: "No server has a usable feed." }));
  it("marks unassigned scenes conservatively", () =>
    expect(
      switchingNow({
        ...input(),
        status: { ...status, current_scene: { uuid: "manual", name: "Just chatting" } },
      }).text,
    ).toMatch(/may be under manual control/));
  it("does not infer manual control before assignments have loaded", () =>
    expect(
      switchingNow({
        ...input(),
        assignmentsKnown: false,
        status: { ...status, current_scene: { uuid: "manual", name: "Just chatting" } },
      }).text,
    ).not.toMatch(/manual/));
  it("recognizes server overrides as automatic scenes", () => {
    const data = input();
    data.servers = data.servers.map((server) => ({
      ...server,
      overrides: { ...server.overrides, live: "alternate" },
    }));
    expect(
      switchingNow({
        ...data,
        status: { ...status, current_scene: { uuid: "alternate", name: "Alternate" } },
      }).text,
    ).not.toMatch(/manual/);
  });
  it("recognizes Starting when switch-to-live is enabled", () =>
    expect(
      switchingNow({
        ...input(),
        status: { ...status, current_scene: { uuid: "starting", name: "Starting" } },
      }).text,
    ).not.toMatch(/manual/));
  it("does not infer manual control when OBS omits UUIDs", () =>
    expect(
      switchingNow({ ...input(), status: { ...status, current_scene: { name: "IRL" } } }).text,
    ).not.toMatch(/manual/));
  it("handles empty servers, rules and samples", () =>
    expect(switchingNow({ ...input(), servers: [], rules: [], samples: [] }).text).toBe(
      "NOALBS reports a LIVE feed.",
    ));
  it("does not invent a reason for an idle state while streaming", () =>
    expect(switchingNow({ ...input(), status: { ...status, state: "idle" } }).text).toMatch(
      /Waiting for the current feed state/,
    ));
});
