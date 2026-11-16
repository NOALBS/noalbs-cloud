import type { SwitcherStatus } from "@/api/types";
import type { Point } from "@/live/reducer";
import { isStale } from "@/live/reducer";

export function streamState(status: SwitcherStatus | null | undefined, now = Date.now()) {
  if (
    !status?.obs_connected ||
    !status.browser_source ||
    !Number.isFinite(Date.parse(status.updated_at)) ||
    isStale(status, now)
  )
    return "unknown";
  return status.streaming ? "live" : "idle";
}

export function hasFeed(sample: Point | undefined, now: number) {
  return Boolean(
    sample && Number.isFinite(sample.at) && now - sample.at <= 45_000 && sample.state !== "offline",
  );
}
