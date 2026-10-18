import type { FeedState, SwitcherStatus } from "@/api/types";

export interface StateLook {
  label: string;
  /** Text colour class. */
  text: string;
  /** Soft background and border classes for pills and cards. */
  soft: string;
  border: string;
  /** Solid fill, for dots and lines. */
  fill: string;
  pulse: boolean;
}

const LOOKS: Record<FeedState | "disconnected" | "obs_offline", StateLook> = {
  live: {
    label: "LIVE",
    text: "text-live",
    soft: "bg-live/12",
    border: "border-live/35",
    fill: "bg-live",
    pulse: true,
  },
  low: {
    label: "LOW BITRATE",
    text: "text-low",
    soft: "bg-low/12",
    border: "border-low/40",
    fill: "bg-low",
    pulse: false,
  },
  offline: {
    label: "OFFLINE",
    text: "text-offline",
    soft: "bg-offline/14",
    border: "border-offline/45",
    fill: "bg-offline",
    pulse: false,
  },
  idle: {
    label: "NOT STREAMING",
    text: "text-muted-foreground",
    soft: "bg-muted",
    border: "border-border",
    fill: "bg-idle",
    pulse: false,
  },
  disconnected: {
    label: "NOT CONNECTED",
    text: "text-muted-foreground",
    soft: "bg-muted",
    border: "border-border",
    fill: "bg-idle",
    pulse: false,
  },
  obs_offline: {
    label: "OBS DISCONNECTED",
    text: "text-low",
    soft: "bg-low/12",
    border: "border-low/40",
    fill: "bg-low",
    pulse: false,
  },
};

/** How to show a switcher's overall state. */
export function lookFor(status: SwitcherStatus | null | undefined): StateLook {
  if (!status) return LOOKS.disconnected;
  if (!status.obs_connected) return LOOKS.obs_offline;
  return LOOKS[status.state];
}

export function lookForState(state: FeedState): StateLook {
  return LOOKS[state];
}
