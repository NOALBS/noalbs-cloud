import { plural, t } from "@lingui/core/macro";
import type { BelaboxView } from "@/lib/belabox";
import type { MoblinView } from "@/lib/moblin";

export function belaboxSummary(view: BelaboxView, configured: boolean): string {
  switch (view.kind) {
    case "online":
      return t`Connected`;
    case "invalid_key":
      return t`Remote key rejected`;
    case "offline":
      return t`Encoder offline`;
    case "connecting":
      return t`Connecting…`;
    case "loading":
      return configured ? t`Loading status…` : t`Not set up`;
    case "idle":
      return configured ? t`Not connected` : t`Not set up`;
  }
}

export function moblinSummary(view: MoblinView, configured: boolean): string {
  switch (view.kind) {
    case "connected":
      return t`Connected`;
    case "paused":
      return t`Connected · paused`;
    case "loading":
      return configured ? t`Loading status…` : t`Not set up`;
    case "disconnected":
      return configured ? t`Not connected` : t`Not set up`;
  }
}

export function connectSummary(
  count: number | undefined,
  online: boolean | undefined,
  error = false,
): string {
  if (count === undefined) return error ? t`Could not load computers` : t`Loading computers…`;
  if (count === 0) return t`No computers paired`;
  if (online === undefined)
    return plural(count, {
      one: "# paired computer · checking status",
      other: "# paired computers · checking status",
    });
  return online
    ? plural(count, { one: "# paired computer · online", other: "# paired computers · online" })
    : plural(count, { one: "# paired computer · offline", other: "# paired computers · offline" });
}
