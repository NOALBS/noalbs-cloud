/** A box on the OBS canvas, in canvas pixels, from its top left. */
export interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Canvas {
  width: number;
  height: number;
}

export type Corner = "top_left" | "top_right" | "bottom_left" | "bottom_right";

/** The smallest a box gets by resizing, in canvas pixels. */
export const MIN_SIZE = 16;

/** How close (a share of the canvas width) an edge or center gets before it snaps. */
export const SNAP_SHARE = 0.015;

export interface Moved {
  box: Box;
  /** The canvas lines it snapped to, for drawing guides. */
  guides: { x: number[]; y: number[] };
}

/** The offset that brings the closest of `points` onto one of `lines`, if within `threshold`. */
function snap(points: number[], lines: number[], threshold: number) {
  let best: { offset: number; line: number } | null = null;
  for (const point of points) {
    for (const line of lines) {
      const offset = line - point;
      if (Math.abs(offset) <= threshold && (!best || Math.abs(offset) < Math.abs(best.offset))) {
        best = { offset, line };
      }
    }
  }
  return best;
}

/**
 * `position` kept so a `length` box stays on a `room`-long side, when it fits:
 * its corners stay on screen to be grabbed. A bigger one moves freely.
 */
function kept(position: number, length: number, room: number): number {
  return length <= room ? Math.min(Math.max(position, 0), room - length) : position;
}

/**
 * `start` moved by `dx`, `dy` canvas pixels, its edges and center snapping to
 * the canvas's edges and center lines when within `threshold`, and kept on
 * screen while it fits.
 */
export function moved(
  start: Box,
  dx: number,
  dy: number,
  canvas: Canvas,
  threshold = canvas.width * SNAP_SHARE,
): Moved {
  const x = start.x + dx;
  const y = start.y + dy;
  const snapX = snap(
    [x, x + start.width / 2, x + start.width],
    [0, canvas.width / 2, canvas.width],
    threshold,
  );
  const snapY = snap(
    [y, y + start.height / 2, y + start.height],
    [0, canvas.height / 2, canvas.height],
    threshold,
  );
  return {
    box: {
      ...start,
      x: kept(x + (snapX?.offset ?? 0), start.width, canvas.width),
      y: kept(y + (snapY?.offset ?? 0), start.height, canvas.height),
    },
    guides: { x: snapX ? [snapX.line] : [], y: snapY ? [snapY.line] : [] },
  };
}

/**
 * `start` resized by dragging `corner` by `dx`, `dy` canvas pixels; the
 * opposite corner stays put. `keepShape` keeps its proportions. With the
 * `canvas`, a box on screen stops growing at its edge.
 */
export function resized(
  start: Box,
  corner: Corner,
  dx: number,
  dy: number,
  keepShape: boolean,
  canvas?: Canvas,
): Box {
  const left = corner === "top_left" || corner === "bottom_left";
  const top = corner === "top_left" || corner === "top_right";
  // Room up to the screen's edge on the side being pulled, if it's on screen now.
  const inside =
    canvas &&
    start.x >= 0 &&
    start.y >= 0 &&
    start.x + start.width <= canvas.width &&
    start.y + start.height <= canvas.height;
  const roomX = inside ? (left ? start.x + start.width : canvas.width - start.x) : Infinity;
  const roomY = inside ? (top ? start.y + start.height : canvas.height - start.y) : Infinity;
  let width = start.width + (left ? -dx : dx);
  let height = start.height + (top ? -dy : dy);
  if (keepShape) {
    // The average of how much each side was pulled, applied to both.
    const scale = Math.min(
      Math.max(
        (width / start.width + height / start.height) / 2,
        MIN_SIZE / Math.min(start.width, start.height),
      ),
      roomX / start.width,
      roomY / start.height,
    );
    width = start.width * scale;
    height = start.height * scale;
  } else {
    width = Math.min(Math.max(MIN_SIZE, width), roomX);
    height = Math.min(Math.max(MIN_SIZE, height), roomY);
  }
  return {
    x: left ? start.x + start.width - width : start.x,
    y: top ? start.y + start.height - height : start.y,
    width,
    height,
  };
}

/**
 * The box to start editing from: where the source is, or, when OBS reports no
 * size for it yet (nothing playing), a quarter of the screen in the middle.
 */
export function startingBox(box: Box, canvas: Canvas): { box: Box; guessed: boolean } {
  if (box.width >= 1 && box.height >= 1) return { box: rounded(box), guessed: false };
  const width = Math.round(canvas.width / 2);
  const height = Math.round(canvas.height / 2);
  return {
    box: {
      x: Math.round((canvas.width - width) / 2),
      y: Math.round((canvas.height - height) / 2),
      width,
      height,
    },
    guessed: true,
  };
}

/** Whole pixels, at least 1 wide and high. */
export function rounded(box: Box): Box {
  return {
    x: Math.round(box.x),
    y: Math.round(box.y),
    width: Math.max(1, Math.round(box.width)),
    height: Math.max(1, Math.round(box.height)),
  };
}

export type Fit = "fit" | "fill" | "stretch" | "original";
export type Anchor =
  | "top_left"
  | "top"
  | "top_right"
  | "left"
  | "center"
  | "right"
  | "bottom_left"
  | "bottom"
  | "bottom_right";

/** Where along a side an anchor sits: 0 at its start, 0.5 in the middle, 1 at its end. */
function along(anchor: Anchor): { x: number; y: number } {
  const x = anchor.endsWith("left") ? 0 : anchor.endsWith("right") ? 1 : 0.5;
  const y = anchor.startsWith("top") ? 0 : anchor.startsWith("bottom") ? 1 : 0.5;
  return { x, y };
}

/** A source's size once cropped; `null` while OBS doesn't know it. */
export function croppedSize(
  source: { width: number; height: number },
  crop: { left: number; right: number; top: number; bottom: number },
): { width: number; height: number } | null {
  const width = source.width - crop.left - crop.right;
  const height = source.height - crop.top - crop.bottom;
  return width >= 1 && height >= 1 ? { width, height } : null;
}

/**
 * Where a source of `size` shows in `box`, as OBS draws it: as large as fits
 * (keeping its shape, pinned to `anchor`), covering the box (cut to it), or
 * stretched. Unknown size: the whole box.
 */
export function inside(
  box: Box,
  size: { width: number; height: number } | null,
  fit: Exclude<Fit, "original">,
  anchor: Anchor = "center",
): Box {
  if (!size || fit !== "fit") return box;
  const scale = Math.min(box.width / size.width, box.height / size.height);
  const width = size.width * scale;
  const height = size.height * scale;
  const at = along(anchor);
  return {
    x: box.x + (box.width - width) * at.x,
    y: box.y + (box.height - height) * at.y,
    width,
    height,
  };
}

export interface Preset {
  fit: Fit;
  anchor: Anchor;
  /** Of the screen, 5 to 100. */
  sizePercent: number;
  /** Of the screen's height, from the edges it's pinned to. */
  marginPercent: number;
}

/**
 * Where a preset puts a source of `size` (cropped) on `canvas`, as the API
 * has OBS do it: `box` is the box it's sized to, `content` where it shows.
 * `null` for its own size while that isn't known.
 */
export function presetPlacement(
  preset: Preset,
  canvas: Canvas,
  size: { width: number; height: number } | null,
): { box: Box; content: Box } | null {
  const full = preset.sizePercent === 100 && preset.fit !== "original";
  const margin = full ? 0 : (canvas.height * preset.marginPercent) / 100;
  const at = along(preset.anchor);
  // The point it's pinned to, `margin` in from the edges it's on.
  const pinX = at.x === 0 ? margin : at.x === 1 ? canvas.width - margin : canvas.width / 2;
  const pinY = at.y === 0 ? margin : at.y === 1 ? canvas.height - margin : canvas.height / 2;
  let width: number;
  let height: number;
  if (preset.fit === "original") {
    if (!size) return null;
    ({ width, height } = size);
  } else {
    width = (canvas.width * preset.sizePercent) / 100;
    height = (canvas.height * preset.sizePercent) / 100;
  }
  const box = { x: pinX - width * at.x, y: pinY - height * at.y, width, height };
  return {
    box,
    content: preset.fit === "original" ? box : inside(box, size, preset.fit, preset.anchor),
  };
}
