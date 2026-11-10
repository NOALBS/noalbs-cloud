import { t } from "@lingui/core/macro";
import type { StreamSummary } from "@/api/types";
import { StateBar } from "@/components/history/state-bar";
import { formatDuration, stateShares } from "@/lib/history";
import { historySummary } from "@/lib/history-summary";

export function HistorySummary({
  streams,
  partial,
}: {
  streams: StreamSummary[];
  partial: boolean;
}) {
  const total = historySummary(streams);
  const shares = stateShares(total);
  const incomplete =
    partial &&
    streams.every((stream) => Date.parse(stream.started_at) >= Date.now() - 30 * 86400000);
  const metrics = [
    { label: t`Streams`, value: String(total.count) },
    { label: t`Time live`, value: formatDuration(total.duration) },
    { label: t`LIVE`, value: `${(shares.live * 100).toFixed(1)}%`, tone: "text-live" },
    { label: t`LOW`, value: `${(shares.low * 100).toFixed(1)}%`, tone: "text-low" },
    { label: t`BRB`, value: `${(shares.offline * 100).toFixed(1)}%`, tone: "text-offline" },
    { label: t`Auto switches`, value: String(total.switches) },
  ];
  return (
    <section
      aria-label={t`Last 30 days`}
      className="flex flex-col gap-4 rounded-2xl border bg-card p-5 shadow-tile"
    >
      <h2 className="font-display text-sm font-semibold">{t`Last 30 days`}</h2>
      <div className="grid grid-cols-2 gap-5 sm:grid-cols-3 lg:grid-cols-6">
        {metrics.map((metric) => (
          <div key={metric.label}>
            <p className="text-xs text-muted-foreground">{metric.label}</p>
            <p className={`mt-1 font-display text-2xl font-bold ${metric.tone ?? ""}`}>
              {metric.value}
            </p>
          </div>
        ))}
      </div>
      <StateBar seconds={total} />
      <p className="text-xs text-muted-foreground">
        {incomplete
          ? t`Loaded streams started in the last 30 days; load older streams to complete this summary.`
          : t`Streams started in the last 30 days, within your plan’s history retention.`}
      </p>
    </section>
  );
}
