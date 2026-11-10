import { t } from "@lingui/core/macro";
import { formatKbps, formatUptime } from "@/lib/live-format";
import { problemLine, streamingMetrics } from "./format";
import type { OperatorItem } from "./model";

export function phoneReading(item: OperatorItem, now: number): string {
  if (item.paused) return t`Paused`;
  if (!item.obs_connected) return t`No OBS`;
  if (item.attention === "no_feed") {
    const since = item.attention_since ?? item.state_since;
    return since ? formatUptime(since, now).replace(/^0(?=\d:)/, "") : "—";
  }
  const { bitrate } = streamingMetrics(item);
  return bitrate != null ? formatKbps(bitrate) : "—";
}
export function phoneDetail(item: OperatorItem, now: number): string {
  const problem = problemLine({ ...item, stops_at: null }, now);
  if (problem) return problem;
  if (item.paused) return t`Over the plan’s switcher limit`;
  if (item.streaming && item.scene && item.stream_started_at) {
    const uptime = formatUptime(item.stream_started_at, now);
    return t`On ${item.scene} · ${uptime}`;
  }
  return item.scene ? t`In OBS: ${item.scene}` : t`No scene`;
}
