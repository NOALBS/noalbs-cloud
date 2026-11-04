import { t } from "@lingui/core/macro";
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
  const withPc = <T extends { label: string }>(result: T): T => {
    if (!status?.pc) return result;
    const label = {
      unknown: t`OBS state unknown`,
      not_running: t`OBS not running`,
      starting: t`OBS starting`,
      connected: t`OBS connected`,
      not_responding: t`OBS not responding`,
      waiting_for_input: t`OBS is waiting for input`,
      safe_mode: t`OBS is in safe mode`,
      crashed: t`OBS crashed`,
      websocket_disabled: t`OBS WebSocket is disabled`,
    }[status.pc.obs];
    return { ...result, label: t`${label} · ${result.label}` };
  };
  if (switcher.paused || status?.paused) {
    return withPc({
      label: t`Paused · over the plan's switcher limit`,
      dot: "bg-idle",
      tag: "PAUSED",
      tagClass: "text-low",
    });
  }
  if (!status) {
    const tag = { tag: "OFF", tagClass: "text-muted-foreground", dot: "bg-idle" };
    return withPc(
      switcher.last_seen_at
        ? { label: t`Offline · last seen ${formatRelative(switcher.last_seen_at)}`, ...tag }
        : { label: t`Never connected`, ...tag },
    );
  }
  if (!status.obs_connected) {
    return withPc({
      label: t`OBS disconnected`,
      dot: "bg-low",
      tag: t`NO OBS`,
      tagClass: "text-low",
    });
  }
  switch (status.state) {
    case "live":
      return withPc({ label: "LIVE", dot: "bg-live", tag: "LIVE", tagClass: "text-live" });
    case "low":
      return withPc({ label: t`Low bitrate`, dot: "bg-low", tag: "LOW", tagClass: "text-low" });
    case "offline":
      return withPc({
        label: t`Feed offline`,
        dot: "bg-offline",
        tag: "BRB",
        tagClass: "text-offline",
      });
    case "idle":
      return withPc({
        label: t`Connected · not streaming`,
        dot: "bg-primary",
        tag: "IDLE",
        tagClass: "text-muted-foreground",
      });
  }
}

export function StatusDot({ className }: { className?: string }) {
  return <span aria-hidden="true" className={cn("size-2 shrink-0 rounded-full", className)} />;
}
