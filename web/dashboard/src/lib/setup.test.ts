import { describe, expect, it } from "vitest";
import { setupProgress } from "./setup";

const scenes = { live: "a", low: "b", offline: "c" };

describe("setupProgress", () => {
  it("is complete once connected, with a server and all three scenes", () => {
    expect(setupProgress({ connected: true, serverCount: 1, scenes })).toMatchObject({
      done: 3,
      complete: true,
    });
  });

  it("counts a missing scene as an unfinished step", () => {
    const progress = setupProgress({
      connected: true,
      serverCount: 2,
      scenes: { ...scenes, offline: null },
    });
    expect(progress).toMatchObject({ scenesSet: false, done: 2, complete: false });
  });

  it("starts at zero", () => {
    const progress = setupProgress({
      connected: false,
      serverCount: 0,
      scenes: { live: null, low: null, offline: null },
    });
    expect(progress.done).toBe(0);
  });
});
