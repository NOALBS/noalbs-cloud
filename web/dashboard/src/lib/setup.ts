import type { Scenes } from "@/api/types";

/** The three setup steps: OBS connected once, a stream server, the LIVE/LOW/BRB scenes. */
export function setupProgress({
  connected,
  serverCount,
  scenes,
}: {
  connected: boolean;
  serverCount: number;
  scenes: Pick<Scenes, "live" | "low" | "offline">;
}) {
  const steps = {
    connected,
    hasServer: serverCount > 0,
    scenesSet: scenes.live != null && scenes.low != null && scenes.offline != null,
  };
  const done = Object.values(steps).filter(Boolean).length;
  return { ...steps, done, total: 3, complete: done === 3 };
}
