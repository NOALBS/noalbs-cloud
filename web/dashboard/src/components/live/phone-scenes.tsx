import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { obsScenesQuery, queryKeys } from "@/api/queries";
import type { FeedState, ObsScenes, Scenes } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { errorMessage } from "@/lib/errors";
import { sceneKey, useHiddenScenes } from "@/lib/hidden-scenes";
import { cn } from "@/lib/utils";
import { lookForState } from "./feed-state";
import { PhoneSheet } from "./phone";

export function PhoneScenes({
  path,
  roles,
  currentUuid,
  currentName,
  canControl,
  obsConnected,
  streaming,
  feedState,
}: {
  path: { workspace_id: string; switcher_id: string };
  roles: Scenes;
  currentUuid: string | null | undefined;
  currentName: string | null | undefined;
  canControl: boolean;
  obsConnected: boolean;
  streaming: boolean;
  feedState: FeedState;
}) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState("");
  const [hidden] = useHiddenScenes(path.switcher_id);
  const client = useQueryClient();
  const query = useQuery({
    ...obsScenesQuery(path.workspace_id, path.switcher_id),
    enabled: obsConnected,
  });
  const switchScene = useMutation({
    scope: { id: `scene-switch-${path.switcher_id}` },
    mutationFn: (uuid: string) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scene", {
          params: { path },
          body: { scene_uuid: uuid },
        }),
      ),
    onSuccess: () => setOpen(false),
    onError: (error) => toast.error(errorMessage(error)),
    onSettled: () =>
      client.invalidateQueries({
        queryKey: queryKeys.obsScenes(path.workspace_id, path.switcher_id),
      }),
  });
  const roleNames: Record<keyof Scenes, string> = {
    live: t`LIVE`,
    low: t`LOW`,
    offline: t`BRB`,
    starting: t`Starting`,
    ending: t`Ending`,
    privacy: t`Privacy`,
    refresh: t`Refresh`,
  };
  const roleOf = (uuid: string | null | undefined) =>
    Object.entries(roles).find(([, value]) => uuid != null && value === uuid)?.[0] as
      | keyof Scenes
      | undefined;
  const all = query.data?.scenes ?? [];
  const visible = all.filter((scene) => !hidden.includes(sceneKey(scene))).slice(0, 6);
  const matches = all.filter((scene) =>
    scene.name.toLocaleLowerCase().includes(filter.trim().toLocaleLowerCase()),
  );
  const look = lookForState(feedState);
  function sceneButton(scene: ObsScenes["scenes"][number], list: boolean) {
    const current =
      scene.uuid != null
        ? scene.uuid === (currentUuid ?? query.data?.current_scene_uuid)
        : scene.name === (currentName ?? query.data?.current_scene_name);
    const role = roleOf(scene.uuid);
    return (
      <button
        key={sceneKey(scene)}
        type="button"
        disabled={!canControl || !obsConnected || !scene.uuid || switchScene.isPending}
        onClick={() => scene.uuid && switchScene.mutate(scene.uuid)}
        aria-pressed={current}
        className={cn(
          "relative flex min-w-0 gap-2 border-b border-divider p-3 text-left disabled:cursor-default",
          list
            ? "min-h-12 w-full items-center justify-between"
            : "min-h-20 flex-col items-start justify-between border-r",
          current && streaming && look.soft,
        )}
      >
        <span className="text-sm font-medium wrap-anywhere">{scene.name}</span>
        {current ? (
          <span
            className={cn(
              "shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium",
              streaming ? `${look.soft} ${look.text}` : "bg-muted text-muted-foreground",
            )}
          >
            {streaming ? t`On air` : t`In OBS`}
          </span>
        ) : (
          <span className="text-[11px] text-muted-foreground">
            {role ? roleNames[role] : t`No role`}
          </span>
        )}
        {current && (
          <span
            className={cn("absolute inset-x-0 bottom-0 h-0.5", streaming ? look.fill : "bg-idle")}
          />
        )}
      </button>
    );
  }
  if (!obsConnected)
    return (
      <p className="py-4 text-sm text-muted-foreground">
        <Trans>OBS isn’t connected.</Trans>
      </p>
    );
  if (query.isPending)
    return (
      <p className="py-4 text-sm text-muted-foreground">
        <Trans>Loading scenes…</Trans>
      </p>
    );
  if (query.isError)
    return (
      <p className="py-4 text-sm text-muted-foreground">
        <Trans>Couldn’t load scenes.</Trans>{" "}
        <button type="button" className="text-primary" onClick={() => void query.refetch()}>
          <Trans>Retry</Trans>
        </button>
      </p>
    );
  return (
    <div className="-mx-3">
      <div className="grid grid-cols-2">{visible.map((scene) => sceneButton(scene, false))}</div>
      {!visible.length && (
        <p className="p-4 text-sm text-muted-foreground">
          {all.length ? t`All scenes are hidden.` : t`No scenes yet.`}
        </p>
      )}
      {all.length > 0 && (
        <PhoneSheet
          open={open}
          onOpenChange={(value) => {
            setOpen(value);
            if (!value) setFilter("");
          }}
          title={t`Scenes`}
          description={t`Tap a scene to switch it in OBS.`}
          trigger={
            <Button variant="ghost" className="min-h-11 w-full">
              <Trans>All {all.length} scenes</Trans>
            </Button>
          }
        >
          <Input
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
            placeholder={t`Filter scenes`}
            aria-label={t`Filter scenes`}
            className="mb-3"
          />
          {[true, false].map((withRole) => {
            const group = matches.filter((scene) => Boolean(roleOf(scene.uuid)) === withRole);
            return (
              group.length > 0 && (
                <div key={String(withRole)}>
                  <h3 className="py-2 text-[10px] font-medium tracking-wide text-muted-foreground uppercase">
                    {withRole ? t`With a role` : t`Other`}
                  </h3>
                  {group.map((scene) => sceneButton(scene, true))}
                </div>
              )
            );
          })}
          {!matches.length && (
            <p className="py-4 text-sm text-muted-foreground">
              <Trans>No matching scenes.</Trans>
            </p>
          )}
        </PhoneSheet>
      )}
    </div>
  );
}
