import type { StreamSummary } from "@/api/types";

export function historySummary(streams: StreamSummary[], now = Date.now()) {
  const cutoff = now - 30 * 24 * 60 * 60 * 1000;
  const recent = streams.filter((stream) => {
    const at = Date.parse(stream.started_at);
    return at >= cutoff && at <= now;
  });
  return recent.reduce(
    (total, stream) => ({
      count: total.count + 1,
      duration: total.duration + stream.duration_seconds,
      live_seconds: total.live_seconds + stream.live_seconds,
      low_seconds: total.low_seconds + stream.low_seconds,
      offline_seconds: total.offline_seconds + stream.offline_seconds,
      switches: total.switches + stream.auto_switches,
    }),
    { count: 0, duration: 0, live_seconds: 0, low_seconds: 0, offline_seconds: 0, switches: 0 },
  );
}
