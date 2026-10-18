import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useReducer } from "react";
import { queryKeys } from "@/api/queries";
import type {
  BelaboxStatus,
  MoblinStatus,
  StatsSample,
  SwitcherEvent,
  SwitcherStatus,
  SwitcherSummary,
} from "@/api/types";
import { initialLiveState, liveReducer } from "./reducer";

/**
 * Subscribes to a switcher's live feed (server-sent events): status, BELABOX, Moblin, a stats
 * sample per second (the last two minutes are replayed on connect) and events.
 * The browser reconnects by itself; the replay fills any gap.
 */
export function useSwitcherLive(workspaceId: string, switcherId: string) {
  const [state, dispatch] = useReducer(liveReducer, initialLiveState);
  const queryClient = useQueryClient();

  useEffect(() => {
    const source = new EventSource(`/api/workspaces/${workspaceId}/switchers/${switcherId}/live`, {
      withCredentials: true,
    });

    source.onopen = () => dispatch({ type: "connection", connection: "open" });
    source.onerror = () => dispatch({ type: "connection", connection: "reconnecting" });

    source.addEventListener("status", (message) => {
      const status = JSON.parse(message.data) as SwitcherStatus | null;
      dispatch({ type: "status", status });
      // Keep the switcher list (sidebar tags, setup progress) current too.
      queryClient.setQueryData<SwitcherSummary[]>(queryKeys.switchers(workspaceId), (current) =>
        current?.map((item) => (item.id === switcherId ? { ...item, status } : item)),
      );
    });
    source.addEventListener("belabox", (message) => {
      dispatch({ type: "belabox", status: JSON.parse(message.data) as BelaboxStatus | null });
    });
    source.addEventListener("moblin", (message) => {
      dispatch({ type: "moblin", status: JSON.parse(message.data) as MoblinStatus | null });
    });
    source.addEventListener("stats", (message) => {
      dispatch({ type: "stats", sample: JSON.parse(message.data) as StatsSample });
    });
    source.addEventListener("event", (message) => {
      const event = JSON.parse(message.data) as SwitcherEvent;
      dispatch({ type: "event", event });
      // Scene lists and settings can change with these.
      if (event.kind === "obs_connected" || event.kind === "scene_switched") {
        void queryClient.invalidateQueries({
          queryKey: queryKeys.obsScenes(workspaceId, switcherId),
        });
      }
      if (event.kind === "auto_switch_changed") {
        void queryClient.invalidateQueries({
          queryKey: queryKeys.switcher(workspaceId, switcherId),
        });
      }
    });

    return () => source.close();
  }, [workspaceId, switcherId, queryClient]);

  return state;
}
