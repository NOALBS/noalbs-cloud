import { SCENE_ROLES } from "@/lib/scene-roles";
import type { Zone } from "@/lib/triggers";
import { cn } from "@/lib/utils";

/**
 * A bar showing which values lead to which scene (from the mockup): green
 * LIVE, amber LOW, red BRB. Colours always come with the labels around it.
 */
export function TriggerScale({
  zones,
  ticks,
  label,
}: {
  zones: Zone[];
  ticks: string[];
  label: string;
}) {
  return (
    <div className="flex flex-col gap-2">
      <div
        role="img"
        aria-label={label}
        className="flex h-4.5 overflow-hidden rounded-full bg-muted"
      >
        {zones.map((zone) => (
          <span
            key={zone.role}
            className={cn("h-full transition-[width]", SCENE_ROLES[zone.role].bar)}
            style={{ width: `${zone.to - zone.from}%` }}
          />
        ))}
      </div>
      <div className="flex justify-between font-mono text-[11px] text-muted-foreground tabular">
        {ticks.map((tick) => (
          <span key={tick}>{tick}</span>
        ))}
      </div>
    </div>
  );
}
