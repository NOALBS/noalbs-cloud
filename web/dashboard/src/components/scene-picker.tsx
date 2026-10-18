import type { ObsScenes } from "@/api/types";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";

const NOT_SET = "none";

/** Picks an OBS scene (by UUID), or none. */
export function ScenePicker({
  label,
  value,
  obs,
  disabled,
  onChange,
  noneLabel = "Not set",
  className,
}: {
  label: string;
  value: string | null;
  obs: ObsScenes | undefined;
  disabled: boolean;
  onChange: (uuid: string | null) => void;
  noneLabel?: string;
  className?: string;
}) {
  const scenes = obs?.scenes.filter((scene) => scene.uuid) ?? [];
  const known = value != null && scenes.some((scene) => scene.uuid === value);

  return (
    <Select
      value={value ?? NOT_SET}
      onValueChange={(next) => onChange(next === NOT_SET ? null : next)}
      disabled={disabled}
    >
      <SelectTrigger className={cn("w-full sm:w-56", className)} aria-label={`Scene for ${label}`}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={NOT_SET}>{noneLabel}</SelectItem>
        {value != null && !known && (
          <SelectItem value={value}>{obs ? "Scene not in OBS" : "Set"}</SelectItem>
        )}
        {scenes.map((scene) => (
          <SelectItem key={scene.uuid} value={scene.uuid as string}>
            {scene.name}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
