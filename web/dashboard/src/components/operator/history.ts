import { useEffect, useState } from "react";
import type { OperatorItem } from "./model";

export type BitratePoint = { at: number; bitrate: number; server?: string };
export type BitrateHistory = Record<string, BitratePoint[]>;
export const HISTORY_MS = 120_000;

export function collectBitrates(
  previous: BitrateHistory,
  items: OperatorItem[],
  at: number,
): BitrateHistory {
  const next: BitrateHistory = {};
  for (const [id, points] of Object.entries(previous)) {
    const fresh = points.filter((point) => point.at >= at - HISTORY_MS && point.at <= at);
    if (fresh.length) next[id] = fresh;
  }
  for (const item of items) {
    const bitrate = item.server?.bitrate;
    if (!item.connected || bitrate == null || !Number.isFinite(bitrate) || bitrate < 0) continue;
    const server = item.server?.id;
    // A switch to another server starts a new line rather than joining two servers' numbers.
    const points = (next[item.switcher_id] ?? []).filter((point) => point.server === server);
    if (!points.some((point) => point.at === at)) points.push({ at, bitrate, server });
    next[item.switcher_id] = points;
  }
  return next;
}

export function useBitrateHistory(items: OperatorItem[], updatedAt: number) {
  const [history, setHistory] = useState<BitrateHistory>({});
  useEffect(() => {
    if (updatedAt > 0) setHistory((previous) => collectBitrates(previous, items, updatedAt));
  }, [items, updatedAt]);
  return history;
}
