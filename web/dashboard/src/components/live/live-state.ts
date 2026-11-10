import type { SwitcherStatus } from "@/api/types";

export type LiveState =
  | "connecting"
  | "reconnecting"
  | "firstRun"
  | "suspended"
  | "paused"
  | "browserOffline"
  | "obsUnreachable"
  | "notStreaming"
  | "noFeed"
  | "low"
  | "live";
export interface LiveStateInput {
  status: Pick<SwitcherStatus, "obs_connected" | "streaming" | "state"> | null | undefined;
  connection: string;
  paused: boolean;
  suspended: boolean;
  setupProgress: { complete: boolean } | null;
}
export function liveState({
  status,
  connection,
  paused,
  suspended,
  setupProgress,
}: LiveStateInput): LiveState {
  if (status === undefined) return "connecting";
  if (connection === "reconnecting") return "reconnecting";
  if (setupProgress && !setupProgress.complete && !status?.streaming) return "firstRun";
  if (suspended) return "suspended";
  if (paused) return "paused";
  if (status === null) return "browserOffline";
  if (!status.obs_connected) return "obsUnreachable";
  if (!status.streaming) return "notStreaming";
  if (status.state === "offline") return "noFeed";
  if (status.state === "low") return "low";
  return "live";
}
