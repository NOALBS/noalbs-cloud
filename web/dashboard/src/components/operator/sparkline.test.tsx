import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { HISTORY_MS } from "./history";
import { Sparkline } from "./sparkline";

const now = 200_000;
function drawing(points: { at: number; bitrate: number }[]) {
  const container = document.createElement("div");
  container.innerHTML = renderToStaticMarkup(<Sparkline points={points} now={now} />);
  return container;
}
describe("operator sparkline", () => {
  it("always draws a full-width, inset hairline for empty or single-point series", () => {
    for (const points of [[], [{ at: now, bitrate: 0 }]]) {
      const chart = drawing(points);
      expect(chart.querySelector("path")?.getAttribute("d")).toBe("M0 36 H200");
      expect(chart.querySelector("polyline")).toBeNull();
    }
  });
  it("retains the two-minute axis and keeps zero/peak strokes inside the viewport", () => {
    const chart = drawing([
      { at: now - HISTORY_MS, bitrate: 100 },
      { at: now - 5000, bitrate: 0 },
      { at: now, bitrate: 100 },
    ]);
    expect(chart.querySelector("polyline")?.getAttribute("points")).toBe(
      "0,6 191.66666666666669,36 200,6",
    );
    expect(chart.querySelector("polyline")?.getAttribute("vector-effect")).toBe(
      "non-scaling-stroke",
    );
  });
  it("ignores expired, future and invalid readings", () => {
    const chart = drawing([
      { at: now - HISTORY_MS - 1, bitrate: 1 },
      { at: now + 1, bitrate: 1 },
      { at: now, bitrate: Number.NaN },
      { at: now, bitrate: -1 },
    ]);
    expect(chart.querySelector("polyline")).toBeNull();
    expect(chart.querySelector("path")).not.toBeNull();
  });
});
