import type { SceneRole } from "./triggers";

/** Colours and labels for the automatic scene roles, used consistently everywhere. */
export const SCENE_ROLES: Record<
  SceneRole,
  { label: string; dot: string; bar: string; text: string }
> = {
  live: { label: "LIVE", dot: "bg-live", bar: "bg-live", text: "text-live" },
  low: { label: "LOW", dot: "bg-low", bar: "bg-low", text: "text-low" },
  offline: { label: "BRB", dot: "bg-offline", bar: "bg-offline", text: "text-offline" },
};
