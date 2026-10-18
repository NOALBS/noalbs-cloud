import { useQuery } from "@tanstack/react-query";
import { Check, Copy, Download } from "lucide-react";
import { useState } from "react";
import { streamMarkersQuery } from "@/api/queries";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { downloadText } from "@/lib/download";
import { errorMessage } from "@/lib/errors";
import { formatOffset, markersToText } from "@/lib/history";

/** A stream's markers, to copy or save as one "time description" line each. */
export function MarkersPanel({
  workspaceId,
  switcherId,
  streamId,
  startedAt,
}: {
  workspaceId: string;
  switcherId: string;
  streamId: string;
  startedAt: string;
}) {
  const markers = useQuery(streamMarkersQuery(workspaceId, switcherId, streamId));
  const [copied, setCopied] = useState(false);
  const list = markers.data ?? [];
  const text = markersToText(list);

  const copy = async () => {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <section aria-label="Markers" className="flex flex-col gap-3 rounded-2xl border bg-card p-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-sm text-muted-foreground">
          Times are from the start of the stream. Add them with the Marker button on the Live page
          or <span className="font-mono">!marker</span> in chat.
        </p>
        <div className="flex gap-2">
          <Button size="sm" variant="outline" disabled={list.length === 0} onClick={copy}>
            {copied ? <Check /> : <Copy />}
            {copied ? "Copied" : "Copy"}
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={list.length === 0}
            onClick={() =>
              downloadText(`markers-${startedAt.slice(0, 10)}.txt`, `${text}\n`, "text/plain")
            }
          >
            <Download />
            Download
          </Button>
        </div>
      </div>

      {markers.isPending && <Skeleton className="h-24" />}
      {markers.isError && <p className="text-sm text-destructive">{errorMessage(markers.error)}</p>}
      {markers.isSuccess && list.length === 0 && (
        <p className="rounded-xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
          No markers on this stream.
        </p>
      )}
      {list.length > 0 && (
        <ul className="divide-y rounded-xl border">
          {list.map((marker) => (
            <li key={marker.id} className="flex items-baseline gap-4 px-4 py-2.5 text-sm">
              <span className="w-16 shrink-0 font-mono text-muted-foreground tabular">
                {formatOffset(marker.offset_seconds)}
              </span>
              <span className="min-w-0 flex-1 break-words">
                {marker.description ?? <span className="text-muted-foreground">Marker</span>}
              </span>
              {marker.created_by && (
                <span className="hidden shrink-0 text-xs text-muted-foreground sm:inline">
                  {marker.created_by}
                </span>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
