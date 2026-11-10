import { expect, it } from "vitest";
import { type LiveStateInput, liveState } from "./live-state";

const base: LiveStateInput = {
  status: { obs_connected: true, streaming: true, state: "live" },
  connection: "connected",
  paused: false,
  suspended: false,
  setupProgress: { complete: true },
};
it("derives states in design precedence order", () => {
  expect(
    liveState({ ...base, status: undefined, connection: "reconnecting", suspended: true }),
  ).toBe("connecting");
  expect(
    liveState({ ...base, connection: "reconnecting", setupProgress: { complete: false } }),
  ).toBe("reconnecting");
  expect(
    liveState({ ...base, status: null, setupProgress: { complete: false }, suspended: true }),
  ).toBe("firstRun");
  expect(liveState({ ...base, suspended: true, paused: true, status: null })).toBe("suspended");
  expect(liveState({ ...base, paused: true, status: null })).toBe("paused");
  expect(liveState({ ...base, status: null })).toBe("browserOffline");
  expect(
    liveState({ ...base, status: { obs_connected: false, streaming: false, state: "offline" } }),
  ).toBe("obsUnreachable");
  expect(
    liveState({ ...base, status: { obs_connected: true, streaming: false, state: "offline" } }),
  ).toBe("notStreaming");
  expect(
    liveState({ ...base, status: { obs_connected: true, streaming: true, state: "offline" } }),
  ).toBe("noFeed");
  expect(
    liveState({ ...base, status: { obs_connected: true, streaming: true, state: "low" } }),
  ).toBe("low");
  expect(liveState(base)).toBe("live");
  expect(liveState({ ...base, setupProgress: null })).toBe("live");
});

it("keeps the normal layout available while streaming with incomplete setup", () => {
  for (const [state, expected] of [
    ["live", "live"],
    ["low", "low"],
    ["offline", "noFeed"],
  ] as const) {
    expect(
      liveState({
        ...base,
        status: { obs_connected: true, streaming: true, state },
        setupProgress: { complete: false },
      }),
    ).toBe(expected);
  }
  expect(
    liveState({
      ...base,
      status: { obs_connected: true, streaming: false, state: "idle" },
      setupProgress: { complete: false },
    }),
  ).toBe("firstRun");
});
