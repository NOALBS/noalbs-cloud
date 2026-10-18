import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { Check, Copy, Eye, EyeOff, Search } from "lucide-react";
import { useMemo, useState } from "react";
import { type Range, streamRawQuery } from "@/api/queries";
import type { StreamSource } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { errorMessage } from "@/lib/errors";
import { flattenNumbers, formatClock, nearest, numericFields } from "@/lib/history";
import { currentLocale } from "@/lib/locale";
import { cn } from "@/lib/utils";
import { HistoryChart, TimeAxis } from "./history-chart";

/** Fields worth charting first when nothing is picked yet, most useful first. */
const SUGGESTED = [/bitrate/i, /rtt/i, /loss|drop/i, /retrans/i, /latency/i, /bandwidth|\bbw/i];
const MAX_PICKED = 6;

/**
 * Everything the stream server reported, over the stream: pick any numeric
 * field to chart it, and read the full snapshot at any moment. Snapshots can
 * hold stream names or the publisher's address, so they stay masked until
 * revealed (the dashboard may be on stream).
 */
export function RawExplorer({
  workspaceId,
  switcherId,
  streamId,
  sources,
  range,
  from,
  to,
  hoverAt,
  onHover,
  onZoom,
}: {
  workspaceId: string;
  switcherId: string;
  streamId: string;
  sources: StreamSource[];
  range: Range | undefined;
  from: number;
  to: number;
  hoverAt: number | null;
  onHover: (at: number | null) => void;
  onZoom: (from: number, to: number) => void;
}) {
  const [serverId, setServerId] = useState(sources[0]?.server_id);
  const [picked, setPicked] = useState<string[] | null>(null);
  const [search, setSearch] = useState("");
  const [revealed, setRevealed] = useState(false);
  const [copied, setCopied] = useState(false);

  const raw = useQuery({
    ...streamRawQuery(workspaceId, switcherId, streamId, serverId ?? "", range),
    enabled: serverId != null,
    placeholderData: keepPreviousData,
  });

  const points = useMemo(
    () =>
      (raw.data?.points ?? []).map((point) => ({
        at: Date.parse(point.at),
        raw: point.raw,
        numbers: flattenNumbers(point.raw),
      })),
    [raw.data],
  );
  const fields = useMemo(() => numericFields(points.map((point) => point.raw)), [points]);
  const suggested = [
    ...new Set(SUGGESTED.flatMap((pattern) => fields.find((field) => pattern.test(field)) ?? [])),
  ].slice(0, 3);
  const shown = picked ?? (suggested.length > 0 ? suggested : fields.slice(0, 1));
  const stepMs = (raw.data?.step_seconds ?? 10) * 1000;
  const matching = fields.filter((field) => field.toLowerCase().includes(search.toLowerCase()));

  const snapshot = hoverAt != null ? nearest(points, hoverAt) : points[points.length - 1];
  const json = snapshot ? JSON.stringify(snapshot.raw, null, 2) : "";

  const toggle = (field: string) => {
    const current = shown;
    setPicked(
      current.includes(field)
        ? current.filter((f) => f !== field)
        : [...current, field].slice(-MAX_PICKED),
    );
  };

  const copy = async () => {
    await navigator.clipboard.writeText(json);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  if (sources.length === 0) {
    return (
      <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
        No stream server reported stats during this stream.
      </p>
    );
  }

  return (
    <div className="grid grid-cols-1 gap-4 lg:grid-cols-12">
      <section
        aria-label="Fields"
        className="flex flex-col gap-3 rounded-2xl border bg-card p-4 lg:col-span-4"
      >
        {sources.length > 1 && (
          <Select
            value={serverId}
            onValueChange={(value) => {
              setServerId(value);
              setPicked(null);
            }}
          >
            <SelectTrigger aria-label="Stream server" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {sources.map((source) => (
                <SelectItem key={source.server_id} value={source.server_id}>
                  {source.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}
        <div className="relative">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder="Find a field"
            aria-label="Find a field"
            className="pl-8"
          />
        </div>
        <p className="text-xs text-muted-foreground">
          {fields.length} numeric {fields.length === 1 ? "field" : "fields"} · pick up to{" "}
          {MAX_PICKED} to chart
        </p>
        <ul className="flex max-h-72 flex-col overflow-y-auto overscroll-contain lg:max-h-[28rem]">
          {raw.isPending && <Skeleton className="h-32" />}
          {matching.map((field) => {
            const on = shown.includes(field);
            return (
              <li key={field}>
                <button
                  type="button"
                  onClick={() => toggle(field)}
                  aria-pressed={on}
                  className={cn(
                    "flex min-h-9 w-full items-center gap-2 rounded-lg px-2 text-left font-mono text-xs break-all hover:bg-surface-2",
                    on && "text-primary",
                  )}
                >
                  <span
                    className={cn(
                      "flex size-4 shrink-0 items-center justify-center rounded border",
                      on && "border-primary bg-primary text-primary-foreground",
                    )}
                  >
                    {on && <Check className="size-3" />}
                  </span>
                  {field}
                </button>
              </li>
            );
          })}
        </ul>
      </section>

      <div className="flex min-w-0 flex-col gap-4 lg:col-span-8">
        {raw.isError && (
          <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
            {errorMessage(raw.error)}
          </p>
        )}
        {raw.isSuccess && points.length === 0 && (
          <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
            This server didn’t report any stats during this stream.
          </p>
        )}

        {shown.map((field) => {
          const series = points.map((point) => ({
            at: point.at,
            value: point.numbers[field] ?? null,
          }));
          const at = hoverAt != null ? nearest(points, hoverAt) : undefined;
          const value = at?.numbers[field];
          return (
            <section key={field} className="flex flex-col gap-2 rounded-2xl border bg-card p-4">
              <div className="flex items-baseline justify-between gap-3">
                <h3 className="min-w-0 font-mono text-xs break-all">{field}</h3>
                <span className="shrink-0 font-mono text-sm font-semibold tabular">
                  {value != null ? value.toLocaleString(currentLocale()) : "—"}
                </span>
              </div>
              <HistoryChart
                from={from}
                to={to}
                stepMs={stepMs}
                lines={[{ points: series, tone: "text-primary" }]}
                hoverAt={hoverAt}
                onHover={onHover}
                onZoom={onZoom}
                minMax={0}
                height={90}
              />
              <TimeAxis from={from} to={to} />
            </section>
          );
        })}

        {snapshot && (
          <section aria-label="Snapshot" className="flex flex-col rounded-2xl border bg-card">
            <div className="flex flex-wrap items-center justify-between gap-2 border-b border-divider px-4 py-2.5">
              <h3 className="text-sm font-semibold">
                Snapshot{" "}
                <span className="font-mono font-normal text-muted-foreground tabular">
                  {formatClock(snapshot.at, true)}
                </span>
              </h3>
              <div className="flex items-center gap-1.5">
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setRevealed(!revealed)}
                  aria-pressed={revealed}
                >
                  {revealed ? <EyeOff /> : <Eye />}
                  {revealed ? "Hide" : "Show"}
                </Button>
                <Button size="sm" variant="outline" onClick={copy}>
                  {copied ? <Check /> : <Copy />}
                  {copied ? "Copied" : "Copy"}
                </Button>
              </div>
            </div>
            {revealed ? (
              <pre className="max-h-96 overflow-auto p-4 font-mono text-xs leading-relaxed">
                {json}
              </pre>
            ) : (
              <p className="px-4 py-8 text-center text-sm text-muted-foreground">
                Hidden, since raw stats can include stream names or addresses. Copy works without
                showing it.
              </p>
            )}
          </section>
        )}
      </div>
    </div>
  );
}
