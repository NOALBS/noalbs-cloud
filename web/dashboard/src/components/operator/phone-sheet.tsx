import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Layers } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { obsScenesQuery, queryKeys } from "@/api/queries";
import { FixButton, StreamButton, useObsControls } from "@/components/live/quick-actions";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { useNow } from "@/hooks/use-now";
import { errorMessage } from "@/lib/errors";
import { organizationRoleLabels } from "@/lib/organizations";
import { roleLabels } from "@/lib/permissions";
import { cn } from "@/lib/utils";
import { isStale } from "@/live/reducer";
import { useSwitcherLive } from "@/live/use-switcher-live";
import { problemLine, stateChip } from "./format";
import { canUseRowActions, type OperatorItem } from "./model";
import { OperatorName } from "./operator-name";

export function PhoneSheet({
  item,
  multi,
  onClose,
}: {
  item: OperatorItem;
  multi: boolean;
  onClose: () => void;
}) {
  const now = useNow();
  const chip = stateChip(item);
  const role = item.role
    ? roleLabels()[item.role]
    : item.organization_role
      ? organizationRoleLabels()[item.organization_role]
      : null;
  return (
    <Sheet
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <SheetContent
        side="bottom"
        className="max-h-[85dvh] gap-3 overflow-y-auto rounded-t-2xl pb-[max(1rem,env(safe-area-inset-bottom))]"
      >
        <SheetHeader className="pb-0 pr-10">
          <SheetTitle className="text-sm">
            <OperatorName
              workspace={item.workspace_name}
              switcher={item.switcher_name}
              multi={multi}
            />
            {role && <span className="text-xs font-normal text-muted-foreground"> · {role}</span>}
          </SheetTitle>
          <SheetDescription className="flex flex-wrap items-baseline gap-2 text-xs">
            <span className={cn("rounded px-1.5 py-0.5 font-semibold", chip.className)}>
              {chip.label}
            </span>
            {problemLine(item, now) ?? t`Control this switcher without leaving the list.`}
          </SheetDescription>
        </SheetHeader>
        <div className="flex flex-col gap-3 px-4">
          {canUseRowActions(item) && <PhoneControls item={item} />}
          <Button asChild>
            <Link
              to="/$workspace/s/$switcher/live"
              params={{ workspace: item.workspace_slug, switcher: item.switcher_id }}
              onClick={onClose}
            >
              <Trans>Open Live</Trans>
            </Link>
          </Button>
        </div>
      </SheetContent>
    </Sheet>
  );
}
function PhoneControls({ item }: { item: OperatorItem }) {
  const live = useSwitcherLive(item.workspace_id, item.switcher_id);
  const now = useNow();
  const status = live.status && !isStale(live.status, now) ? live.status : null;
  const enabled =
    canUseRowActions(item) &&
    live.connection === "open" &&
    Boolean(status?.obs_connected) &&
    !status?.paused;
  const path = { workspace_id: item.workspace_id, switcher_id: item.switcher_id };
  const controls = useObsControls(path);
  const [showScenes, setShowScenes] = useState(false);
  const scenes = useQuery({
    ...obsScenesQuery(item.workspace_id, item.switcher_id),
    enabled: showScenes && enabled,
  });
  const client = useQueryClient();
  const switchScene = useMutation({
    scope: { id: `scene-switch-${item.switcher_id}` },
    mutationFn: (uuid: string) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scene", {
          params: { path },
          body: { scene_uuid: uuid },
        }),
      ),
    onError: (error) => toast.error(errorMessage(error)),
    onSuccess: () => setShowScenes(false),
    onSettled: () =>
      client.invalidateQueries({
        queryKey: queryKeys.obsScenes(item.workspace_id, item.switcher_id),
      }),
  });
  return (
    <>
      <div className="grid grid-cols-2 gap-2 [&>button]:w-full [&>button]:justify-center [&>button]:text-sm">
        <FixButton controls={controls} disabled={!enabled} variant="compact" />
        <Button
          variant="secondary"
          disabled={!enabled}
          aria-expanded={showScenes}
          aria-controls="operator-sheet-scenes"
          onClick={() => setShowScenes(!showScenes)}
        >
          <Layers className="size-4" />
          <Trans>Switch scene</Trans>
        </Button>
      </div>
      {showScenes && (
        <section
          id="operator-sheet-scenes"
          aria-label={t`Scenes`}
          className="flex flex-col gap-1 rounded-lg border p-2"
        >
          {scenes.isPending ? (
            <p className="p-2 text-xs text-muted-foreground">
              <Trans>Loading scenes…</Trans>
            </p>
          ) : scenes.isError ? (
            <p role="alert" className="p-2 text-xs text-destructive">
              {errorMessage(scenes.error)}
            </p>
          ) : scenes.data?.scenes.length === 0 ? (
            <p className="p-2 text-xs text-muted-foreground">
              <Trans>No scenes in OBS</Trans>
            </p>
          ) : (
            scenes.data?.scenes.map((scene) => (
              <Button
                key={scene.uuid ?? scene.name}
                variant="ghost"
                className="justify-start"
                disabled={!enabled || !scene.uuid || switchScene.isPending}
                onClick={() => {
                  if (enabled && scene.uuid) switchScene.mutate(scene.uuid);
                }}
              >
                {scene.name}
              </Button>
            ))
          )}
        </section>
      )}
      {item.streaming && (
        <div className="[&>button]:w-full [&>button]:justify-center [&>button]:text-sm">
          {status?.streaming ? (
            <StreamButton
              controls={controls}
              status={status}
              disabled={!enabled}
              variant="compact"
            />
          ) : (
            <Button variant="secondary" disabled>
              <Trans>Stop stream</Trans>
            </Button>
          )}
        </div>
      )}
    </>
  );
}
