import { useMemo } from "react";
import { LOGO_PIECES } from "@/components/logo";
import { currentLocale } from "@/lib/locale";

/** The backdrop's canvas in SVG units; it's scaled to cover the screen, cropping the sides. */
const WIDTH = 1600;
const HEIGHT = 1000;
/** Roughly one item per cell, in a brick pattern, nudged so it doesn't look like a grid. */
const CELL = 108;

type Item =
  | { kind: "logo"; x: number; y: number; angle: number; size: number }
  | {
      kind: "camera";
      x: number;
      y: number;
      angle: number;
      size: number;
      /** Two kinds of action camera, after the usual shapes: a long one and a boxy one. */
      model: "bullet" | "box";
    }
  | { kind: "kbps"; x: number; y: number; angle: number; kbps: number };

/** A repeatable random series, so the backdrop is the same on every visit. */
function seeded(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state / 2 ** 32;
  };
}

/** Bitrates as the switcher sees them: mostly LIVE, some LOW, the odd dropout. */
const READINGS = [6180, 5940, 6420, 5710, 6050, 6300, 5820, 2180, 720, 0];

function scatter(): Item[] {
  const random = seeded(11);
  const between = (min: number, max: number) => min + random() * (max - min);
  const items: Item[] = [];
  for (let row = 0; row * CELL < HEIGHT + CELL; row++) {
    // Readings are wider than a cell: never two side by side.
    let wide: boolean = false;
    for (let col = -1; col * CELL < WIDTH + CELL; col++) {
      const x = (col + 0.5 + (row % 2) * 0.5) * CELL + between(-24, 24);
      const y = (row + 0.5) * CELL + between(-24, 24);
      const angle = between(-40, 40);
      const pick = random();
      const wasWide: boolean = wide;
      wide = pick >= 0.84 && !wasWide;
      if (pick < 0.66 || (wasWide && pick >= 0.84))
        items.push({ kind: "logo", x, y, angle, size: between(28, 52) });
      else if (pick < 0.84)
        items.push({
          kind: "camera",
          x,
          y,
          angle,
          size: between(0.42, 0.56),
          model: random() < 0.5 ? "bullet" : "box",
        });
      else
        items.push({
          kind: "kbps",
          x,
          y,
          angle: angle / 3,
          kbps: READINGS[Math.floor(random() * READINGS.length)] ?? 0,
        });
    }
  }
  return items;
}

/**
 * A long action camera like Sony's AS300, seen from the front left: the long side,
 * the top with the record button, and the big lens barrel standing out of the
 * narrow front. Centred on 0,0, about 88 × 50 units.
 */
function BulletCamera() {
  return (
    // An oblique box seen from the front left. From the corner where side, top and
    // front meet (0,0), the side runs back to (100,-22) and down to (0,66), and the
    // front and top recede to (-30,-12). Scaled to the size of the other camera, with
    // the stroke widened by as much so both draw alike.
    <g transform="scale(0.62) translate(-28 -16)" strokeWidth="5.4">
      {/* The outline, rounded at every corner. */}
      <path d="M-30 -6 Q-30 -12 -24.14 -13.29 L64.14 -32.71 Q70 -34 75.57 -31.77 L91.64 -25.34 Q100 -22 100 -13 L100 35 Q100 44 91.21 45.94 L5.86 64.71 Q0 66 -5.57 63.77 L-24.43 56.23 Q-30 54 -30 48 Z" />
      {/* Where the side meets the top and the front. */}
      <path d="M92 -20.2 L0 0 L0 62 M0 0 L-26 -10.4" className="fill-none" />
      {/* The record button, lying flat on the top near the front. */}
      <rect
        x="-4.5"
        y="-3"
        width="9"
        height="6"
        rx="3"
        transform="matrix(1.205 -0.265 -1.034 -0.414 11.5 -11.8)"
        className="fill-offline stroke-none"
      />
      {/* The lens barrel standing out of the front, high up: its back rim, the sides
          it sweeps forward (tangent to the rim), then its face with the glass. */}
      <ellipse cx="-15" cy="16.5" rx="22.7" ry="14.4" transform="rotate(70.7 -15 16.5)" />
      <path d="M-21.44 -5.25 L-34.7 -2.33 L-21.82 41.17 L-8.56 38.25 Z" className="stroke-none" />
      <path d="M-21.44 -5.25 L-34.7 -2.33 M-8.56 38.25 L-21.82 41.17" className="fill-none" />
      <ellipse cx="-28.26" cy="19.42" rx="22.7" ry="14.4" transform="rotate(70.7 -28.26 19.42)" />
      <ellipse cx="-28.26" cy="19.42" rx="17.4" ry="11" transform="rotate(70.7 -28.26 19.42)" />
      <ellipse
        cx="-28.26"
        cy="19.42"
        rx="10.6"
        ry="6.7"
        transform="rotate(70.7 -28.26 19.42)"
        className="fill-muted-foreground stroke-none"
      />
    </g>
  );
}

/**
 * A boxy action camera from the front: a screen beside a big lens with a ridged
 * ring, the record button on top and the mount's fingers below. Centred on 0,0,
 * about 80 × 70 units.
 */
function BoxCamera() {
  return (
    <>
      <rect x="-30" y="-34" width="12" height="6" rx="3" className="fill-offline stroke-none" />
      <rect x="-22" y="27" width="12" height="7" rx="2" />
      <rect x="10" y="27" width="12" height="7" rx="2" />
      <rect x="-39" y="-29" width="78" height="57" rx="10" />
      <rect x="-33" y="-21" width="19" height="29" rx="3" />
      <rect x="-30" y="15" width="11" height="5" rx="2.5" />
      <circle cx="15" cy="-2" r="21" strokeDasharray="2.5 2.5" />
      <circle cx="15" cy="-2" r="16" />
      <circle cx="15" cy="-2" r="7" className="fill-muted-foreground stroke-none" />
      <path d="M7 -8 a9 9 0 0 1 5 -4" className="fill-none" />
    </>
  );
}

/** The colour a reading would get on the Live page (LOW below 2,500, BRB below 300). */
const tone = (kbps: number) =>
  kbps < 300 ? "fill-offline" : kbps < 2500 ? "fill-low" : "fill-live";

/**
 * The login page's backdrop: NOALBS logos at all angles, with action cameras and
 * bitrate readings among them. Decorative and static; it runs to every edge and
 * the sign-in card sits on top.
 */
export function LoginBackdrop() {
  const items = useMemo(scatter, []);

  return (
    <div aria-hidden="true" className="pointer-events-none fixed inset-0 -z-10 overflow-hidden">
      <svg
        aria-hidden="true"
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        preserveAspectRatio="xMidYMid slice"
        className="size-full"
      >
        {items.map((item) => {
          const at = `translate(${item.x.toFixed(1)} ${item.y.toFixed(1)}) rotate(${item.angle.toFixed(1)})`;
          const key = `${item.x.toFixed(0)}-${item.y.toFixed(0)}`;
          switch (item.kind) {
            case "logo":
              // The logo's own viewBox is 260 × 262; centre it and scale to `size`.
              return (
                <g
                  key={key}
                  transform={`${at} scale(${(item.size / 260).toFixed(3)}) translate(-130 -131)`}
                  opacity="0.2"
                >
                  {Object.values(LOGO_PIECES).map((d) => (
                    <path
                      key={d}
                      d={d}
                      className="fill-muted-foreground stroke-background"
                      strokeWidth="10"
                      strokeLinejoin="round"
                    />
                  ))}
                </g>
              );
            case "camera":
              return (
                <g
                  key={key}
                  transform={`${at} scale(${item.size.toFixed(2)})`}
                  className="fill-background stroke-muted-foreground"
                  strokeWidth="3.5"
                  strokeLinejoin="round"
                  strokeLinecap="round"
                  opacity="0.35"
                >
                  {item.model === "box" ? <BoxCamera /> : <BulletCamera />}
                </g>
              );
            case "kbps":
              return (
                <text
                  key={key}
                  transform={at}
                  textAnchor="middle"
                  dominantBaseline="middle"
                  fontSize="15"
                  className={`font-mono font-semibold ${tone(item.kbps)}`}
                  opacity="0.35"
                >
                  {item.kbps.toLocaleString(currentLocale())} kbps
                </text>
              );
          }
          return null;
        })}
      </svg>
    </div>
  );
}
