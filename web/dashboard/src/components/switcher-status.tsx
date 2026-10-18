import type { SwitcherSummary } from "@/api/types";
import { formatRelative } from "@/lib/time";
import { cn } from "@/lib/utils";

/**
 * A short status for lists, from the live status if connected: a label, the
 * dot colour, and a terse mono tag (with its text colour) for the sidebar.
 */
export function switcherStatus(
  switcher: Pick<SwitcherSummary, "last_seen_at" | "status"> & { paused?: boolean },
) {
  const status = switcher.status;
  if (switcher.paused || status?.paused) {
    return {
      label: "Paused · over the plan's switcher limit",
      dot: "bg-idle",
      tag: "PAUSED",
      tagClass: "text-low",
    };
  }
  if (!status) {
    const tag = { tag: "OFF", tagClass: "text-muted-foreground", dot: "bg-idle" };
    return switcher.last_seen_at
      ? { label: `Offline · last seen ${formatRelative(switcher.last_seen_at)}`, ...tag }
      : { label: "Never connected", ...tag };
  }
  if (!status.obs_connected) {
    return { label: "OBS disconnected", dot: "bg-low", tag: "NO OBS", tagClass: "text-low" };
  }
  switch (status.state) {
    case "live":
      return { label: "LIVE", dot: "bg-live", tag: "LIVE", tagClass: "text-live" };
    case "low":
      return { label: "Low bitrate", dot: "bg-low", tag: "LOW", tagClass: "text-low" };
    case "offline":
      return { label: "Feed offline", dot: "bg-offline", tag: "BRB", tagClass: "text-offline" };
    case "idle":
      return {
        label: "Connected · not streaming",
        dot: "bg-primary",
        tag: "IDLE",
        tagClass: "text-muted-foreground",
      };
  }
}

export function StatusDot({ className }: { className?: string }) {
  return <span aria-hidden="true" className={cn("size-2 shrink-0 rounded-full", className)} />;
}
