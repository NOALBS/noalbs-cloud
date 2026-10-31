import { t } from "@lingui/core/macro";
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

const looks = (): Record<FeedState | "disconnected" | "obs_offline", StateLook> => ({
  live: {
    label: "LIVE",
    text: "text-live",
    soft: "bg-live/12",
    border: "border-live/35",
    fill: "bg-live",
    pulse: true,
  },
  low: {
    label: t`LOW BITRATE`,
    text: "text-low",
    soft: "bg-low/12",
    border: "border-low/40",
    fill: "bg-low",
    pulse: false,
  },
  offline: {
    label: t`OFFLINE`,
    text: "text-offline",
    soft: "bg-offline/14",
    border: "border-offline/45",
    fill: "bg-offline",
    pulse: false,
  },
  idle: {
    label: t`NOT STREAMING`,
    text: "text-muted-foreground",
    soft: "bg-muted",
    border: "border-border",
    fill: "bg-idle",
    pulse: false,
  },
  disconnected: {
    label: t`NOT CONNECTED`,
    text: "text-muted-foreground",
    soft: "bg-muted",
    border: "border-border",
    fill: "bg-idle",
    pulse: false,
  },
  obs_offline: {
    label: t`OBS DISCONNECTED`,
    text: "text-low",
    soft: "bg-low/12",
    border: "border-low/40",
    fill: "bg-low",
    pulse: false,
  },
});

/** How to show a switcher's overall state. */
export function lookFor(status: SwitcherStatus | null | undefined): StateLook {
  if (!status) return looks().disconnected;
  if (!status.obs_connected) return looks().obs_offline;
  return looks()[status.state];
}

export function lookForState(state: FeedState): StateLook {
  return looks()[state];
}
