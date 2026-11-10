import { plural, t } from "@lingui/core/macro";
import { formatUptime } from "@/lib/live-format";
import type { OperatorCounts, OperatorItem } from "./model";

export function streamingMetrics(item: OperatorItem) {
  const active = item.streaming && !item.paused && item.obs_connected;
  return { bitrate: active ? item.server?.bitrate : null, rtt: active ? item.server?.rtt : null };
}

export function offlineSummary(total: number, counts: OperatorCounts): string {
  if (counts.workspaces != null)
    return plural(total, {
      one: `# switcher in ${plural(counts.workspaces, { one: "# workspace", other: "# workspaces" })} · all offline`,
      other: `# switchers in ${plural(counts.workspaces, { one: "# workspace", other: "# workspaces" })} · all offline`,
    });
  if (counts.clients != null)
    return plural(total, {
      one: `# switcher in ${plural(counts.clients, { one: "# client", other: "# clients" })} · all offline`,
      other: `# switchers in ${plural(counts.clients, { one: "# client", other: "# clients" })} · all offline`,
    });
  return plural(total, { one: "# switcher · all offline", other: "# switchers · all offline" });
}

export function timeAgo(at: string, now: number): string {
  const seconds = Math.max(0, Math.floor((now - Date.parse(at)) / 1000));
  if (seconds < 60) return plural(seconds, { one: "# sec ago", other: "# sec ago" });
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return plural(minutes, { one: "# min ago", other: "# min ago" });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return plural(hours, { one: "# h ago", other: "# h ago" });
  const days = Math.floor(hours / 24);
  return plural(days, { one: "# day ago", other: "# days ago" });
}

export function problemLine(item: OperatorItem, now: number): string | null {
  if (item.attention === "obs_lost" && item.obs_lost) {
    const ago = timeAgo(item.obs_lost.since, now);
    const duration = item.obs_lost.stream_started_at
      ? formatUptime(item.obs_lost.stream_started_at, Date.parse(item.obs_lost.since))
      : null;
    return duration
      ? t`OBS disconnected ${ago} · was live for ${duration}`
      : t`OBS disconnected ${ago}`;
  }
  const since = item.attention_since ?? item.state_since;
  if (item.attention === "no_feed") {
    const duration = since ? formatUptime(since, now) : null;
    const remaining = item.stops_at
      ? formatUptime(new Date(now).toISOString(), Date.parse(item.stops_at))
      : null;
    if (duration && remaining) return t`No feed for ${duration} · the stream stops in ${remaining}`;
    if (duration) return t`No feed for ${duration}`;
    if (remaining) return t`No feed · the stream stops in ${remaining}`;
    return t`No feed`;
  }
  if (item.attention === "low") {
    const duration = since ? formatUptime(since, now) : null;
    return duration ? t`Low bitrate for ${duration}` : t`Low bitrate`;
  }
  return null;
}

export function stateChip(item: OperatorItem): { label: string; className: string } {
  if (item.paused) return { label: t`Paused`, className: "bg-low/10 text-low" };
  if (!item.obs_connected) return { label: t`No OBS`, className: "bg-low/10 text-low" };
  if (!item.streaming)
    return { label: t`Not streaming`, className: "bg-idle/10 text-muted-foreground" };
  if (item.state === "offline") return { label: t`BRB`, className: "bg-offline/10 text-offline" };
  if (item.state === "low") return { label: t`LOW`, className: "bg-low/10 text-low" };
  if (item.state === "live") return { label: t`LIVE`, className: "bg-live/10 text-live" };
  return { label: t`Not streaming`, className: "bg-idle/10 text-muted-foreground" };
}
