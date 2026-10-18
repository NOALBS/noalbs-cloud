import { cn } from "@/lib/utils";

/**
 * The NOALBS "N": three slanted pieces (left pillar, diagonal, right pillar)
 * with a dark outline, redrawn from the app icon (`icons/noalbsicopre2.png`).
 */
export const LOGO_VIEWBOX = "0 0 260 262";
export const LOGO_BLUE = "#5576DA";
export const LOGO_PIECES = {
  left: "M22 62 L90 169 L90 243 L22 243 Z",
  diagonal: "M24 17 L93 17 L238 245 L169 245 Z",
  right: "M169 17 L237 17 L237 204 L169 97 Z",
} as const;

export type LogoPiece = keyof typeof LOGO_PIECES;

/** The static logo. */
export function LogoMark({ className }: { className?: string }) {
  return (
    <svg viewBox={LOGO_VIEWBOX} className={cn("shrink-0", className)} aria-hidden="true">
      {Object.entries(LOGO_PIECES).map(([piece, d]) => (
        <path
          key={piece}
          d={d}
          fill={LOGO_BLUE}
          stroke="#0B0D10"
          strokeWidth="6"
          strokeLinejoin="round"
        />
      ))}
    </svg>
  );
}
