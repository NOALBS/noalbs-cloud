import { Trans } from "@lingui/react/macro";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { streamMarkersQuery, streamsQuery } from "@/api/queries";
import type { Marker, StreamSummary } from "@/api/types";
import { formatOffset } from "@/lib/history";

export function runningStream(streams: StreamSummary[], streaming: boolean) {
  if (!streaming) return undefined;
  return streams
    .filter((stream) => stream.ended_at == null)
    .sort((a, b) => Date.parse(b.started_at) - Date.parse(a.started_at))[0];
}

export function useLiveMarkers(workspaceId: string, switcherId: string, streaming: boolean) {
  const streams = useInfiniteQuery({
    ...streamsQuery(workspaceId, switcherId),
    enabled: streaming,
    refetchInterval: streaming ? 10_000 : false,
  });
  const stream = runningStream(streams.data?.pages.flat() ?? [], streaming);
  const markers = useQuery({
    ...streamMarkersQuery(workspaceId, switcherId, stream?.id ?? ""),
    enabled: Boolean(stream),
    refetchInterval: stream ? 10_000 : false,
  });
  return {
    markers: stream ? markers.data : undefined,
    error: streaming && (streams.isError || Boolean(stream && markers.isError)),
    loading: streaming && (streams.isPending || Boolean(stream && markers.isPending)),
  };
}

export function Markers({
  markers,
  error,
  loading,
}: {
  markers: Marker[] | undefined;
  error: boolean;
  loading: boolean;
}) {
  if (error)
    return (
      <p className="text-sm text-muted-foreground">
        <Trans>Couldn’t load markers.</Trans>
      </p>
    );
  if (loading)
    return (
      <p className="text-sm text-muted-foreground">
        <Trans>Loading…</Trans>
      </p>
    );
  if (!markers?.length)
    return (
      <p className="text-sm text-muted-foreground">
        <Trans>No markers yet</Trans>
      </p>
    );
  return (
    <ol className="divide-y divide-divider">
      {markers.map((marker) => (
        <li key={marker.id} className="flex items-baseline gap-3 py-2.5">
          <time
            dateTime={marker.at}
            className="font-display text-xs text-muted-foreground tabular-nums"
          >
            {formatOffset(marker.offset_seconds)}
          </time>
          <span className="text-sm">{marker.description || <Trans>Marker</Trans>}</span>
        </li>
      ))}
    </ol>
  );
}
