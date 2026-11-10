import { useQuery } from "@tanstack/react-query";
import { connectDevicesQuery } from "@/api/queries";
import { useNow } from "@/hooks/use-now";
import { useSwitcher } from "@/hooks/use-switcher";
import { belaboxView } from "@/lib/belabox";
import { belaboxSummary, connectSummary, moblinSummary } from "@/lib/device-summary";
import { moblinView } from "@/lib/moblin";
import { isStale } from "@/live/reducer";
import { useSwitcherLive } from "@/live/use-switcher-live";

export function useDeviceSummary() {
  const { workspace, switcher } = useSwitcher();
  const live = useSwitcherLive(workspace.id, switcher.id);
  const now = useNow();
  const devices = useQuery(connectDevicesQuery(workspace.id, switcher.id));
  const status = live.status && isStale(live.status, now) ? null : live.status;
  const belabox = belaboxView(live.belabox, now);
  const moblin = moblinView(live.moblin, now);
  return {
    status,
    belabox: belabox.kind === "online" ? belabox.status : undefined,
    moblin: moblin.kind === "connected" || moblin.kind === "paused" ? moblin.status : undefined,
    belaboxText: belaboxSummary(belabox, switcher.belabox.has_remote_key),
    moblinText: moblinSummary(moblin, switcher.moblin.has_password),
    connectText: connectSummary(
      devices.data?.length,
      status === undefined ? undefined : Boolean(status?.pc),
      devices.isError,
    ),
  };
}
