import type { SwitcherStatus } from "@/api/types";
import { type Actor, can, isSupport } from "@/lib/permissions";
import { type Connection, isStale } from "@/live/reducer";

export function compactAvailability(
  actor: Actor,
  status: SwitcherStatus | null | undefined,
  connection: Connection,
  paused: boolean,
  now: number,
) {
  const fresh = status && !isStale(status, now) ? status : null;
  const ready = connection === "open" && !paused && Boolean(fresh?.obs_connected);
  return {
    status: fresh,
    control: can(actor, "switcher.control") && ready,
    manage: can(actor, "switcher.manage") && !isSupport(actor) && ready,
  };
}
