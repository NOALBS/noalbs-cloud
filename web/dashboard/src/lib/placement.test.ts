import { describe, expect, it } from "vitest";
import {
  croppedSize,
  MIN_SIZE,
  moved,
  presetPlacement,
  resized,
  rounded,
  startingBox,
} from "./placement";

const canvas = { width: 1920, height: 1080 };
const box = { x: 100, y: 100, width: 640, height: 360 };

describe("moved", () => {
  it("moves by the drag, away from any line", () => {
    // Right edge at 890 and bottom at 510: not near 960 or 540.
    expect(moved(box, 150, 50, canvas)).toEqual({
      box: { ...box, x: 250, y: 150 },
      guides: { x: [], y: [] },
    });
  });

  it("snaps edges and centers to the screen's edges and center lines", () => {
    // Left edge 10 px from the screen's left: onto it.
    expect(moved(box, -90, 0, canvas).box.x).toBe(0);
    // Right edge 1910 → 1920.
    const right = moved(box, 1170, 0, canvas);
    expect(right.box.x + box.width).toBe(1920);
    expect(right.guides.x).toEqual([1920]);
    // Center 545 px down → the middle line at 540.
    const middle = moved(box, 0, 265, canvas);
    expect(middle.box.y + box.height / 2).toBe(540);
    expect(middle.guides.y).toEqual([540]);
  });
});

describe("resized", () => {
  it("keeps the opposite corner where it is", () => {
    const bigger = resized(box, "bottom_right", 64, 36, true);
    expect(rounded(bigger)).toEqual({ x: 100, y: 100, width: 704, height: 396 });
    const fromTopLeft = resized(box, "top_left", -64, -36, true);
    expect(fromTopLeft.x + fromTopLeft.width).toBe(740);
    expect(fromTopLeft.y + fromTopLeft.height).toBe(460);
  });

  it("keeps its shape unless told not to", () => {
    const shaped = resized(box, "bottom_right", 320, 0, true);
    expect(shaped.width / shaped.height).toBeCloseTo(16 / 9);
    expect(resized(box, "bottom_right", 320, 0, false)).toEqual({ ...box, width: 960 });
  });

  it("doesn't shrink below the smallest size", () => {
    const tiny = resized(box, "bottom_right", -5000, -5000, false);
    expect([tiny.width, tiny.height]).toEqual([MIN_SIZE, MIN_SIZE]);
    const shaped = resized(box, "top_right", -5000, 5000, true);
    expect(Math.min(shaped.width, shaped.height)).toBeCloseTo(MIN_SIZE);
  });
});

describe("startingBox", () => {
  it("starts from where it is, or a guess when OBS knows no size", () => {
    expect(startingBox({ ...box, x: 10.4 }, canvas)).toEqual({
      box: { ...box, x: 10 },
      guessed: false,
    });
    expect(startingBox({ x: 0, y: 0, width: 0, height: 0 }, canvas)).toEqual({
      box: { x: 480, y: 270, width: 960, height: 540 },
      guessed: true,
    });
    expect(rounded({ x: 0.4, y: 0.6, width: 0.2, height: 2.5 })).toEqual({
      x: 0,
      y: 1,
      width: 1,
      height: 3,
    });
  });
});

describe("staying on screen", () => {
  it("stops moves at the edges while the box fits", () => {
    expect(moved(box, 0, 5000, canvas).box.y).toBe(1080 - 360);
    expect(moved(box, -5000, 0, canvas).box.x).toBe(0);
    // Bigger than the screen: moves freely.
    const big = { x: -100, y: -100, width: 2200, height: 1300 };
    expect(moved(big, 0, 500, canvas).box.y).toBe(400);
  });

  it("stops growing at the edge of the screen", () => {
    const grown = resized(box, "bottom_right", 5000, 5000, true, canvas);
    expect(grown.y + grown.height).toBeCloseTo(1080);
    expect(grown.width / grown.height).toBeCloseTo(16 / 9);
    const free = resized(box, "top_left", -5000, -50, false, canvas);
    expect(free.x).toBe(0);
    expect(free.width).toBe(740);
  });
});

describe("presetPlacement", () => {
  const chat = { width: 600, height: 900 };

  it("fits a source into a box of the screen, pinned like OBS", () => {
    const full = presetPlacement(
      { fit: "fit", anchor: "center", sizePercent: 100, marginPercent: 3 },
      canvas,
      chat,
    );
    expect(full?.box).toEqual({ x: 0, y: 0, width: 1920, height: 1080 });
    // 600×900 scaled to 1080 high: 720 wide, in the middle.
    expect(full?.content).toEqual({ x: 600, y: 0, width: 720, height: 1080 });

    const corner = presetPlacement(
      { fit: "fit", anchor: "bottom_right", sizePercent: 25, marginPercent: 3 },
      canvas,
      chat,
    );
    // The API's numbers: a 480×270 box whose bottom right is 32.4 px in.
    expect(corner?.box.x).toBeCloseTo(1887.6 - 480);
    expect(corner?.box.y).toBeCloseTo(1047.6 - 270);
    // Pinned right, so the narrow chat sits at the box's right side.
    expect((corner?.content.x ?? 0) + (corner?.content.width ?? 0)).toBeCloseTo(1887.6);
  });

  it("fills and stretches to the box, or keeps its own size", () => {
    const fill = presetPlacement(
      { fit: "fill", anchor: "center", sizePercent: 100, marginPercent: 0 },
      canvas,
      chat,
    );
    expect(fill?.content).toEqual(fill?.box);
    const original = presetPlacement(
      { fit: "original", anchor: "bottom_left", sizePercent: 100, marginPercent: 3 },
      canvas,
      { width: 420, height: 90 },
    );
    expect(original?.box.x).toBeCloseTo(32.4);
    expect(original?.box.y).toBeCloseTo(1047.6 - 90);
    expect(
      presetPlacement(
        { fit: "original", anchor: "center", sizePercent: 100, marginPercent: 0 },
        canvas,
        null,
      ),
    ).toBeNull();
  });

  it("knows a source's size only once OBS does", () => {
    const none = { left: 0, right: 0, top: 0, bottom: 0 };
    expect(croppedSize({ width: 0, height: 0 }, none)).toBeNull();
    expect(croppedSize({ width: 1920, height: 1080 }, { ...none, bottom: 80 })).toEqual({
      width: 1920,
      height: 1000,
    });
  });
});
