import { describe, expect, it } from "vitest";
import { check, LOOP_SECONDS, readingAt, START, type Switcher } from "./demo";

const run = (seconds: number) => {
  let switcher: Switcher = START;
  const states: string[] = [];
  for (let t = 0; t < seconds; t++) {
    switcher = check(
      switcher,
      readingAt(t, () => 0.5),
    );
    states.push(switcher.state);
  }
  return states;
};

describe("landing demo", () => {
  it("goes LIVE → LOW → offline → LIVE once per loop", () => {
    const states = run(LOOP_SECONDS);
    const changes = states.filter((state, i) => i === 0 || state !== states[i - 1]);
    expect(changes).toEqual(["live", "low", "offline", "live"]);
  });

  it("waits for the retry attempts before getting worse", () => {
    const states = run(LOOP_SECONDS);
    // The hill starts at 10 s; LOW holds two more checks first.
    expect(states.slice(10, 13)).toEqual(["live", "live", "low"]);
  });

  it("recovers from offline at once", () => {
    const states = run(LOOP_SECONDS);
    // Out of the tunnel at 25 s.
    expect(states[24]).toBe("offline");
    expect(states[25]).toBe("live");
  });

  it("loops", () => {
    expect(readingAt(LOOP_SECONDS + 3, () => 0)).toEqual(readingAt(3, () => 0));
  });
});
