import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import type { BelaboxSettingsUpdate } from "@/api/types";
import { errorMessage } from "@/lib/errors";

type Path = { workspace_id: string; switcher_id: string };

const BASE = "/workspaces/{workspace_id}/switchers/{switcher_id}/belabox";

/**
 * The BELABOX settings dialog's requests. They're orders: BELABOX answers a while later
 * (WiFi results, a rescan), which the dialog's polling of the details picks up, so each
 * success only asks for an early refresh.
 */
export function useBelaboxConfig(path: Path) {
  const queryClient = useQueryClient();
  const refresh = () =>
    queryClient.invalidateQueries({
      queryKey: queryKeys.belaboxDetails(path.workspace_id, path.switcher_id),
    });
  const options = {
    onSuccess: refresh,
    onError: (error: unknown) => toast.error(errorMessage(error)),
  };
  const wifi = (device: string) => ({ ...path, device });

  const settings = useMutation({
    mutationFn: (body: BelaboxSettingsUpdate) =>
      unwrap(api.PUT(`${BASE}/settings`, { params: { path }, body })),
    ...options,
  });
  const scanWifi = useMutation({
    mutationFn: () => unwrap(api.POST(`${BASE}/wifi/scan`, { params: { path } })),
    ...options,
  });
  const connectWifi = useMutation({
    mutationFn: (uuid: string) =>
      unwrap(api.POST(`${BASE}/wifi/connect`, { params: { path }, body: { uuid } })),
    ...options,
  });
  const disconnectWifi = useMutation({
    mutationFn: (uuid: string) =>
      unwrap(api.POST(`${BASE}/wifi/disconnect`, { params: { path }, body: { uuid } })),
    ...options,
  });
  const addWifi = useMutation({
    mutationFn: (body: { device: string; ssid: string; password: string | null }) =>
      unwrap(api.POST(`${BASE}/wifi/networks`, { params: { path }, body })),
    ...options,
  });
  const forgetWifi = useMutation({
    mutationFn: (uuid: string) =>
      unwrap(api.DELETE(`${BASE}/wifi/networks/{uuid}`, { params: { path: { ...path, uuid } } })),
    ...options,
  });
  const hotspot = useMutation({
    mutationFn: ({ device, active }: { device: string; active: boolean }) =>
      unwrap(
        api.PUT(`${BASE}/wifi/{device}/hotspot`, {
          params: { path: wifi(device) },
          body: { active },
        }),
      ),
    ...options,
  });
  const hotspotConfig = useMutation({
    mutationFn: ({
      device,
      ...body
    }: {
      device: string;
      name: string;
      password: string | null;
      channel: string;
    }) =>
      unwrap(
        api.PUT(`${BASE}/wifi/{device}/hotspot/config`, {
          params: { path: wifi(device) },
          body,
        }),
      ),
    ...options,
  });
  const modem = useMutation({
    mutationFn: ({
      id,
      ...body
    }: {
      id: string;
      autoconfig: boolean;
      apn: string;
      username: string;
      password: string | null;
      roaming: boolean;
      network: string;
      network_type: string;
    }) =>
      unwrap(
        api.PUT(`${BASE}/modems/{modem_id}`, {
          params: { path: { ...path, modem_id: id } },
          body,
        }),
      ),
    ...options,
  });
  const scanModem = useMutation({
    mutationFn: (id: string) =>
      unwrap(
        api.POST(`${BASE}/modems/{modem_id}/scan`, {
          params: { path: { ...path, modem_id: id } },
        }),
      ),
    ...options,
  });

  return {
    settings,
    scanWifi,
    connectWifi,
    disconnectWifi,
    addWifi,
    forgetWifi,
    hotspot,
    hotspotConfig,
    modem,
    scanModem,
  };
}

export type BelaboxConfig = ReturnType<typeof useBelaboxConfig>;
