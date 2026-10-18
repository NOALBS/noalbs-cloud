import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { SCENE_ROLES } from "@/lib/scene-roles";
import type { SceneRole } from "@/lib/triggers";

/**
 * One trigger: a number with a unit, or off (`null`). Turning it on restores
 * `fallback` so there's always a sensible value to edit.
 */
export function TriggerField({
  id,
  label,
  sceneRole,
  unit,
  value,
  fallback,
  onChange,
  disabled,
}: {
  id: string;
  label: string;
  sceneRole: SceneRole;
  unit: string;
  value: number | null;
  fallback: number;
  onChange: (value: number | null) => void;
  disabled?: boolean;
}) {
  const on = value !== null;

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-2">
        <label htmlFor={id} className="flex items-center gap-2 text-sm font-semibold">
          <span
            className={`size-2.5 rounded-sm ${SCENE_ROLES[sceneRole].dot}`}
            aria-hidden="true"
          />
          {label}
        </label>
        <Switch
          checked={on}
          onCheckedChange={(checked) => onChange(checked ? fallback : null)}
          disabled={disabled}
          aria-label={`${label}: ${on ? "on" : "off"}`}
        />
      </div>
      <div className="flex h-12 items-center rounded-xl border border-input bg-background pr-3 focus-within:ring-[3px] focus-within:ring-ring/50">
        <Input
          id={id}
          type="number"
          inputMode="numeric"
          min={1}
          value={on ? value : ""}
          placeholder="Off"
          disabled={disabled || !on}
          onChange={(event) =>
            onChange(event.target.value === "" ? 0 : Math.round(Number(event.target.value)))
          }
          className="h-full border-0 bg-transparent font-mono text-base tabular shadow-none focus-visible:ring-0 dark:bg-transparent"
        />
        <span className="text-sm text-muted-foreground">{unit}</span>
      </div>
    </div>
  );
}
