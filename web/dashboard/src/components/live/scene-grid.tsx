import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { EyeOff, SlidersHorizontal } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { obsScenesQuery, queryKeys } from "@/api/queries";
import type { ObsScenes, Scenes } from "@/api/types";
import { Skeleton } from "@/components/ui/skeleton";
import { errorMessage } from "@/lib/errors";
import { sceneKey, useHiddenScenes } from "@/lib/hidden-scenes";
import { cn } from "@/lib/utils";

const ROLE_TAGS: [keyof Scenes, string][] = [
  ["live", "LIVE SCENE"],
  ["low", "LOW SCENE"],
  ["offline", "BRB SCENE"],
  ["starting", "STARTING"],
  ["ending", "ENDING"],
  ["privacy", "PRIVACY"],
  ["refresh", "REFRESH"],
];

const TILE =
  "flex min-h-19 flex-col items-start justify-between gap-1.5 rounded-xl p-3 text-left transition-colors";
const HEADER_BUTTON =
  "flex h-8 items-center gap-1.5 rounded-lg px-2 text-xs font-medium hover:bg-surface-2";

/**
 * OBS's scenes as big tap targets; the one on air is highlighted. "Customize"
 * turns the same buttons into show/hide toggles, so each viewer can keep only
 * the scenes they use (remembered in this browser).
 */
export function SceneGrid({
  path,
  scenes,
  currentUuid,
  canControl,
  obsConnected,
}: {
  path: { workspace_id: string; switcher_id: string };
  scenes: Scenes;
  currentUuid: string | null | undefined;
  canControl: boolean;
  obsConnected: boolean;
}) {
  const queryClient = useQueryClient();
  const [hidden, setHidden] = useHiddenScenes(path.switcher_id);
  const [editing, setEditing] = useState(false);
  const list = useQuery({
    ...obsScenesQuery(path.workspace_id, path.switcher_id),
    enabled: obsConnected,
  });

  const switchScene = useMutation({
    mutationFn: (sceneUuid: string) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scene", {
          params: { path },
          body: { scene_uuid: sceneUuid },
        }),
      ),
    onError: (error) => toast.error(errorMessage(error)),
    onSettled: () =>
      queryClient.invalidateQueries({
        queryKey: queryKeys.obsScenes(path.workspace_id, path.switcher_id),
      }),
  });

  const roleOf = (uuid: string | null | undefined) =>
    ROLE_TAGS.find(([key]) => uuid && scenes[key] === uuid)?.[1] ?? "SCENE";

  const hiddenSet = new Set(hidden);
  const all = list.data?.scenes ?? [];
  const isCurrent = (scene: ObsScenes["scenes"][number]) =>
    scene.uuid != null && scene.uuid === currentUuid;
  // The scene on air always shows, hidden or not.
  const visible = all.filter((scene) => !hiddenSet.has(sceneKey(scene)) || isCurrent(scene));
  const hiddenCount = all.length - visible.length;
  const anyHidden = all.some((scene) => hiddenSet.has(sceneKey(scene)));

  const toggle = (key: string) =>
    setHidden(hiddenSet.has(key) ? hidden.filter((item) => item !== key) : [...hidden, key]);

  return (
    <section aria-label="Scenes" className="flex flex-col gap-2.5">
      <div className="flex min-h-8 items-center justify-between gap-2">
        <h2 className="font-semibold">Scenes</h2>
        {list.data &&
          (editing ? (
            <span className="flex items-center gap-1">
              <button
                type="button"
                onClick={() => setHidden(anyHidden ? [] : all.map((scene) => sceneKey(scene)))}
                className={cn(HEADER_BUTTON, "text-muted-foreground hover:text-foreground")}
              >
                {anyHidden ? "Show all" : "Hide all"}
              </button>
              <button
                type="button"
                onClick={() => setEditing(false)}
                className={cn(HEADER_BUTTON, "bg-primary text-primary-foreground hover:bg-primary")}
              >
                Done
              </button>
            </span>
          ) : (
            <span className="flex items-center gap-1 text-xs text-muted-foreground">
              {hiddenCount > 0
                ? `${hiddenCount} hidden`
                : canControl
                  ? "Tap to switch"
                  : "Synced from OBS"}
              <button
                type="button"
                onClick={() => setEditing(true)}
                className={cn(HEADER_BUTTON, "text-primary")}
              >
                <SlidersHorizontal className="size-3.5" />
                Customize
              </button>
            </span>
          ))}
      </div>

      {editing && (
        <p className="text-xs text-muted-foreground">
          Tap scenes to show or hide them here. Only this browser remembers it; the scene on air
          always shows.
        </p>
      )}

      {!obsConnected && (
        <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
          Scenes appear once OBS is connected.
        </p>
      )}
      {obsConnected && list.isPending && <Skeleton className="h-40 rounded-2xl" />}

      {list.data && editing && (
        <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-3">
          {all.map((scene) => {
            const key = sceneKey(scene);
            const shown = !hiddenSet.has(key);
            return (
              <button
                key={key}
                type="button"
                aria-pressed={shown}
                onClick={() => toggle(key)}
                className={cn(
                  TILE,
                  shown
                    ? "border-2 border-primary bg-primary-soft"
                    : "border border-dashed border-border-strong bg-transparent text-muted-foreground opacity-70",
                )}
              >
                <span className="text-sm font-semibold">{scene.name}</span>
                <span className="flex items-center gap-1.5 text-[11px] font-semibold tracking-wider">
                  {shown ? (
                    <span className="text-primary-soft-foreground">SHOWN</span>
                  ) : (
                    <>
                      <EyeOff className="size-3" aria-hidden="true" />
                      HIDDEN
                    </>
                  )}
                </span>
              </button>
            );
          })}
        </div>
      )}

      {list.data && !editing && visible.length > 0 && (
        <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-3">
          {visible.map((scene) => {
            const current = isCurrent(scene);
            const pending = switchScene.isPending && switchScene.variables === scene.uuid;
            return (
              <button
                key={sceneKey(scene)}
                type="button"
                disabled={!canControl || !scene.uuid || current || switchScene.isPending}
                aria-pressed={current}
                onClick={() => scene.uuid && switchScene.mutate(scene.uuid)}
                className={cn(
                  TILE,
                  "disabled:cursor-default",
                  current
                    ? "border-2 border-primary bg-primary-soft"
                    : "border bg-card enabled:hover:bg-surface-2",
                  pending && "animate-pulse",
                )}
              >
                <span className="text-sm font-semibold">{scene.name}</span>
                <span
                  className={cn(
                    "flex items-center gap-1.5 text-[11px] font-semibold tracking-wider",
                    current ? "text-offline" : "text-muted-foreground",
                  )}
                >
                  {current && (
                    <span className="size-1.5 rounded-full bg-offline" aria-hidden="true" />
                  )}
                  {current ? "ON AIR" : roleOf(scene.uuid)}
                </span>
              </button>
            );
          })}
        </div>
      )}

      {list.data && !editing && visible.length === 0 && (
        <button
          type="button"
          onClick={() => setEditing(true)}
          className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground hover:bg-surface-2"
        >
          All scenes are hidden. Tap to pick some.
        </button>
      )}
    </section>
  );
}
