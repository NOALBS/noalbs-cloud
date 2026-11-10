import { describe, expect, it } from "vitest";
import { collectBitrates, HISTORY_MS } from "./history";
import type { OperatorItem } from "./model";

const row = (bitrate: number | null, connected = true): OperatorItem => ({
  switcher_id: "one",
  switcher_name: "One",
  workspace_id: "w",
  workspace_name: "W",
  workspace_slug: "w",
  connected,
  can_control: false,
  obs_connected: true,
  streaming: true,
  recording: false,
  paused: false,
  state: "live",
  server: { id: "s", name: "Server", kind: "belabox", bitrate },
});

describe("rolling bitrate history", () => {
  it("starts empty, accepts zero, deduplicates snapshots and does not mutate previous history", () => {
    expect(collectBitrates({}, [], 1000)).toEqual({});
    const first = collectBitrates({}, [row(0)], 1000);
    const next = collectBitrates(first, [row(100)], 6000);
    expect(first.one).toEqual([{ at: 1000, bitrate: 0 }]);
    expect(next.one).toEqual([
      { at: 1000, bitrate: 0 },
      { at: 6000, bitrate: 100 },
    ]);
    expect(collectBitrates(next, [row(100)], 6000)).toEqual(next);
  });
  it("keeps only the last two minutes, including across paging and absent samples", () => {
    const history = {
      one: [
        { at: 0, bitrate: 1 },
        { at: 5000, bitrate: 2 },
      ],
    };
    expect(collectBitrates(history, [], HISTORY_MS + 1).one).toEqual([{ at: 5000, bitrate: 2 }]);
    expect(collectBitrates(history, [], HISTORY_MS + 5001)).toEqual({});
  });
  it("ignores unavailable, malformed and offline readings", () => {
    for (const entry of [row(null), row(Number.NaN), row(-1), row(Infinity), row(500, false)]) {
      expect(collectBitrates({}, [entry], 1000)).toEqual({});
    }
  });
});
