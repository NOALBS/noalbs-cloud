import { describe, expect, it } from "vitest";
import type { ObsStats } from "@/api/types";
import { bitrateBetween, droppedTone, formatDropped, formatMegabytes } from "./obs-stats";

const reading = (streaming: boolean, bytes: number, ms: number): ObsStats => ({
  streaming,
  reconnecting: false,
  stream_bytes: bytes,
  stream_duration_ms: ms,
  congestion: 0,
  network: { frames: 0, total: 0 },
  rendering: { frames: 0, total: 0 },
  encoding: { frames: 0, total: 0 },
  this_stream: streaming,
  fps: 60,
  frame_render_ms: 1.2,
  cpu_percent: 5,
  memory_mb: 512,
  disk_free_mb: 20_480,
});

describe("bitrateBetween", () => {
  it("counts the bytes sent between two readings", () => {
    // 1.5 MB over 2 s.
    expect(
      bitrateBetween(reading(true, 10_000_000, 60_000), reading(true, 11_500_000, 62_000)),
    ).toBe(6000);
  });

  it("needs two close readings of one running stream", () => {
    const later = reading(true, 11_500_000, 62_000);
    expect(bitrateBetween(reading(false, 0, 0), later)).toBeNull();
    expect(bitrateBetween(later, reading(true, 1000, 2000))).toBeNull();
    expect(bitrateBetween(later, later)).toBeNull();
    expect(bitrateBetween(reading(true, 0, 1000), later)).toBeNull();
  });
});

describe("dropped frames", () => {
  it("reads like OBS's Stats dock", () => {
    expect(formatDropped({ frames: 12, total: 600 })).toBe("12 (2.0%)");
    expect(formatDropped({ frames: 0, total: 0 })).toBe("0 (0.0%)");
  });

  it("shades by how many", () => {
    expect(droppedTone({ frames: 0, total: 600 })).toBe("ok");
    expect(droppedTone({ frames: 3, total: 600 })).toBe("warn");
    expect(droppedTone({ frames: 12, total: 600 })).toBe("bad");
  });
});

describe("formatMegabytes", () => {
  it("picks a unit", () => {
    expect(formatMegabytes(512.4)).toBe("512 MB");
    expect(formatMegabytes(20_480)).toBe("20.0 GB");
    expect(formatMegabytes(2.5 * 1024 * 1024)).toBe("2.5 TB");
  });
});
