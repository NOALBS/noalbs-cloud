import { type StateSeconds, stateShares } from "@/lib/history";
import { cn } from "@/lib/utils";

const PARTS = [
  { key: "live", label: "LIVE", fill: "bg-live", text: "text-live" },
  { key: "low", label: "LOW", fill: "bg-low", text: "text-low" },
  { key: "offline", label: "BRB", fill: "bg-offline", text: "text-offline" },
] as const;

const pct = (share: number) => `${Math.round(share * 100)}%`;

/** How a stream's time split between LIVE, LOW and BRB, as one stacked bar. */
export function StateBar({
  seconds,
  legend = false,
  className,
}: {
  seconds: StateSeconds;
  legend?: boolean;
  className?: string;
}) {
  const shares = stateShares(seconds);
  const empty = shares.live + shares.low + shares.offline === 0;

  return (
    <div className={cn("flex flex-col gap-1.5", className)}>
      <div
        className="flex h-1.5 w-full overflow-hidden rounded-full bg-muted"
        role="img"
        aria-label={
          empty
            ? "No feed recorded"
            : PARTS.map((part) => `${part.label} ${pct(shares[part.key])}`).join(", ")
        }
      >
        {PARTS.map(
          (part) =>
            shares[part.key] > 0 && (
              <span
                key={part.key}
                className={part.fill}
                style={{ width: `${shares[part.key] * 100}%` }}
              />
            ),
        )}
      </div>
      {legend && !empty && (
        <div className="flex flex-wrap gap-x-4 gap-y-1 font-mono text-xs tabular">
          {PARTS.map((part) => (
            <span key={part.key} className="flex items-center gap-1.5 text-muted-foreground">
              <span className={cn("size-2 rounded-full", part.fill)} />
              {part.label}
              <span className={cn("font-semibold", part.text)}>{pct(shares[part.key])}</span>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
