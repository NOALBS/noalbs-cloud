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
 * How long the feed may stay silent while a browser source is connected. obs-link
 * rewrites the status every 10 s, so this is two missed heartbeats: the connection
 * is dead even if the browser hasn't noticed (common on phones changing networks).
 */
const SILENT_FEED_MS = 25_000;

/**
 * Subscribes to a switcher's live feed (server-sent events): status, BELABOX, Moblin, a stats
 * sample per second (the last two minutes are replayed on connect) and events.
 * The browser reconnects by itself; the replay fills any gap. A feed that goes
 * silent is reopened, since a dead connection doesn't always raise an error.
 */
export function useSwitcherLive(workspaceId: string, switcherId: string) {
  const [state, dispatch] = useReducer(liveReducer, initialLiveState);
  const queryClient = useQueryClient();

  useEffect(() => {
    let source: EventSource;
    let heardAt = Date.now();
    // Only a connected browser source keeps the feed busy; without one it can be quiet.
    let expectHeartbeat = false;
    const heard = () => {
      heardAt = Date.now();
    };

    const connect = () => {
      source = new EventSource(`/api/workspaces/${workspaceId}/switchers/${switcherId}/live`, {
        withCredentials: true,
      });

      source.onopen = () => {
        heard();
        dispatch({ type: "connection", connection: "open" });
      };
      source.onerror = () => dispatch({ type: "connection", connection: "reconnecting" });

      source.addEventListener("status", (message) => {
        heard();
        const status = JSON.parse(message.data) as SwitcherStatus | null;
        expectHeartbeat = status !== null;
        dispatch({ type: "status", status });
        // Keep the switcher list (sidebar tags, setup progress) current too.
        queryClient.setQueryData<SwitcherSummary[]>(queryKeys.switchers(workspaceId), (current) =>
          current?.map((item) => (item.id === switcherId ? { ...item, status } : item)),
        );
      });
      source.addEventListener("belabox", (message) => {
        heard();
        dispatch({ type: "belabox", status: JSON.parse(message.data) as BelaboxStatus | null });
      });
      source.addEventListener("moblin", (message) => {
        heard();
        dispatch({ type: "moblin", status: JSON.parse(message.data) as MoblinStatus | null });
      });
      source.addEventListener("stats", (message) => {
        heard();
        dispatch({ type: "stats", sample: JSON.parse(message.data) as StatsSample });
      });
      source.addEventListener("event", (message) => {
        heard();
        const event = JSON.parse(message.data) as SwitcherEvent;
        dispatch({ type: "event", event });
        // Scene lists and settings can change with these.
        if (event.kind === "obs_connected" || event.kind === "scene_switched") {
          void queryClient.invalidateQueries({
            queryKey: queryKeys.obsScenes(workspaceId, switcherId),
          });
        }
        if (
          event.kind === "auto_switch_changed" ||
          event.kind === "preview_off_air_changed" ||
          event.kind === "connect_permissions_changed" ||
          event.kind === "connect_watchdog_changed"
        ) {
          void queryClient.invalidateQueries({
            queryKey: queryKeys.switcher(workspaceId, switcherId),
          });
        }
      });
    };

    connect();
    const watchdog = setInterval(() => {
      if (!expectHeartbeat || Date.now() - heardAt < SILENT_FEED_MS) return;
      dispatch({ type: "connection", connection: "reconnecting" });
      source.close();
      heard();
      connect();
    }, 5_000);

    return () => {
      clearInterval(watchdog);
      source.close();
    };
  }, [workspaceId, switcherId, queryClient]);

  return state;
}
