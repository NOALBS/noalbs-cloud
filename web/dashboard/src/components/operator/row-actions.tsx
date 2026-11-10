import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, Wrench } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { obsScenesQuery, queryKeys } from "@/api/queries";
import { useObsControls } from "@/components/live/quick-actions";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { errorMessage } from "@/lib/errors";
import type { OperatorItem } from "./model";

export function RowActions({ item }: { item: OperatorItem }) {
  const path = { workspace_id: item.workspace_id, switcher_id: item.switcher_id };
  const { fix } = useObsControls(path);
  const [open, setOpen] = useState(false);
  const client = useQueryClient();
  const scenes = useQuery({
    ...obsScenesQuery(item.workspace_id, item.switcher_id),
    enabled: open,
  });
  const switchScene = useMutation({
    scope: { id: `scene-switch-${item.switcher_id}` },
    mutationFn: (sceneUuid: string) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scene", {
          params: { path },
          body: { scene_uuid: sceneUuid },
        }),
      ),
    onError: (error) => toast.error(errorMessage(error)),
    onSettled: () =>
      client.invalidateQueries({
        queryKey: queryKeys.obsScenes(item.workspace_id, item.switcher_id),
      }),
  });
  return (
    <div className="relative z-10 flex items-center gap-1">
      <Button
        variant="secondary"
        size="sm"
        disabled={fix.isPending}
        onClick={() => fix.mutate()}
        title={t`Reconnect the stream sources playing on the scene on air`}
        aria-label={t`Fix feed for ${item.switcher_name}`}
      >
        <Wrench className="size-3" />
        <Trans>Fix feed</Trans>
      </Button>
      <DropdownMenu open={open} onOpenChange={setOpen}>
        <DropdownMenuTrigger asChild>
          <Button
            variant="secondary"
            size="sm"
            aria-label={t`Scenes for ${item.switcher_name}`}
            disabled={switchScene.isPending}
          >
            <Trans>Scenes</Trans>
            <ChevronDown className="size-3" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          {scenes.isPending ? (
            <DropdownMenuItem disabled>
              <Trans>Loading scenes…</Trans>
            </DropdownMenuItem>
          ) : scenes.isError ? (
            <DropdownMenuItem disabled>{errorMessage(scenes.error)}</DropdownMenuItem>
          ) : scenes.data?.scenes.length === 0 ? (
            <DropdownMenuItem disabled>
              <Trans>No scenes in OBS</Trans>
            </DropdownMenuItem>
          ) : (
            scenes.data?.scenes.map((scene) => (
              <DropdownMenuItem
                key={scene.uuid ?? scene.name}
                disabled={!scene.uuid || switchScene.isPending}
                onSelect={() => scene.uuid && switchScene.mutate(scene.uuid)}
              >
                {scene.name}
              </DropdownMenuItem>
            ))
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
