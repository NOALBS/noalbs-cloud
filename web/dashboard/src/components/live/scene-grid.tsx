import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { EyeOff, LayoutGrid, List, Pencil, Plus, SlidersHorizontal, Trash2 } from "lucide-react";
import { type CSSProperties, type FormEvent, type ReactNode, useId, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { obsScenesQuery, queryKeys } from "@/api/queries";
import type { ObsScenes, Scenes } from "@/api/types";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { errorMessage } from "@/lib/errors";
import { sceneKey, useHiddenScenes } from "@/lib/hidden-scenes";
import { cn } from "@/lib/utils";

const roleTags = (): [keyof Scenes, string][] => [
  ["live", t`LIVE`],
  ["low", t`LOW`],
  ["offline", t`BRB`],
  ["starting", t`Starting`],
  ["ending", t`Ending`],
  ["privacy", t`Privacy`],
  ["refresh", t`Refresh`],
];

const TILE =
  "flex min-h-19 flex-col items-start justify-between gap-1.5 rounded-xl p-3 text-left transition-colors";
const HEADER_BUTTON =
  "flex h-8 items-center gap-1.5 rounded-lg px-2 text-xs font-medium hover:bg-surface-2";

export function readSceneView(switcherId: string): "grid" | "list" {
  try {
    return localStorage.getItem(`noalbs-scene-view:${switcherId}`) === "list" ? "list" : "grid";
  } catch {
    return "grid";
  }
}

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
  canManage,
  obsConnected,
  previewUuid,
  onPreviewSelect,
  onProgramSelect,
  operatorTools,
}: {
  path: { workspace_id: string; switcher_id: string };
  scenes: Scenes;
  currentUuid: string | null | undefined;
  canControl: boolean;
  /** May add and remove scenes in OBS (members who manage switchers, not while paused). */
  canManage: boolean;
  obsConnected: boolean;
  previewUuid?: string;
  onPreviewSelect?: (scene: { uuid: string; name: string }) => void;
  onProgramSelect?: (scene: { uuid: string; name: string }) => void;
  operatorTools?: ReactNode;
}) {
  const queryClient = useQueryClient();
  const [hidden, setHidden] = useHiddenScenes(path.switcher_id);
  const [editing, setEditing] = useState(false);
  const [viewState, setViewState] = useState(() => ({
    switcherId: path.switcher_id,
    view: readSceneView(path.switcher_id),
  }));
  const view =
    viewState.switcherId === path.switcher_id ? viewState.view : readSceneView(path.switcher_id);
  const toggleView = () => {
    const next = view === "grid" ? "list" : "grid";
    setViewState({ switcherId: path.switcher_id, view: next });
    try {
      localStorage.setItem(`noalbs-scene-view:${path.switcher_id}`, next);
    } catch {
      // The selection still works when browser storage is unavailable.
    }
  };
  // Adding and removing scenes in OBS: a mode of its own, so a tap meant to
  // switch scenes can't remove one.
  const [managing, setManaging] = useState(false);
  const [creating, setCreating] = useState(false);
  const [removing, setRemoving] = useState<{ uuid: string; name: string } | null>(null);
  const refreshScenes = () =>
    queryClient.invalidateQueries({
      queryKey: queryKeys.obsScenes(path.workspace_id, path.switcher_id),
    });
  const remove = useMutation({
    mutationFn: (scene: { uuid: string; name: string }) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scene/remove", {
          params: { path },
          body: { scene_uuid: scene.uuid },
        }),
      ),
    onMutate: () => setRemoving(null),
    onSuccess: (_, scene) => toast.success(t`Removed the scene ${scene.name}`),
    onError: (error) => toast.error(errorMessage(error)),
    onSettled: refreshScenes,
  });
  const list = useQuery({
    ...obsScenesQuery(path.workspace_id, path.switcher_id),
    enabled: obsConnected,
  });

  const switchScene = useMutation({
    // One at a time, so a second tap can't overtake the first.
    scope: { id: `scene-switch-${path.switcher_id}` },
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
    roleTags().find(([key]) => uuid && scenes[key] === uuid)?.[1] ?? t`No role`;

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
    <section aria-label={t`Scenes`} className="flex flex-col gap-2.5">
      <div className="flex min-h-8 items-center justify-between gap-2">
        <h2 className="font-semibold">
          <Trans>Scenes</Trans>
        </h2>
        {list.data &&
          (managing ? (
            <Button
              variant="ghost"
              type="button"
              onClick={() => setManaging(false)}
              className={cn(HEADER_BUTTON, "bg-primary text-primary-foreground hover:bg-primary")}
            >
              <Trans>Done</Trans>
            </Button>
          ) : editing ? (
            <span className="flex items-center gap-1">
              <Button
                variant="ghost"
                type="button"
                onClick={() => setHidden(anyHidden ? [] : all.map((scene) => sceneKey(scene)))}
                className={cn(HEADER_BUTTON, "text-muted-foreground hover:text-foreground")}
              >
                {anyHidden ? t`Show all` : t`Hide all`}
              </Button>
              <Button
                variant="ghost"
                type="button"
                onClick={() => setEditing(false)}
                className={cn(HEADER_BUTTON, "bg-primary text-primary-foreground hover:bg-primary")}
              >
                <Trans>Done</Trans>
              </Button>
            </span>
          ) : (
            <span className="flex items-center gap-1 text-xs text-muted-foreground">
              {operatorTools}
              <span className="scene-instruction">
                {hiddenCount > 0
                  ? t`${hiddenCount} hidden`
                  : canControl
                    ? onPreviewSelect
                      ? t`1–9 preview · Shift+1–9 cuts`
                      : t`Tap to switch`
                    : t`Synced from OBS`}
              </span>
              <Button
                variant="ghost"
                type="button"
                aria-label={view === "grid" ? t`Show scenes as list` : t`Show scenes as grid`}
                onClick={toggleView}
                className={HEADER_BUTTON}
              >
                {view === "grid" ? (
                  <LayoutGrid className="size-3.5" />
                ) : (
                  <List className="size-3.5" />
                )}
              </Button>
              <Button
                variant="ghost"
                type="button"
                aria-label={t`Customize scenes`}
                onClick={() => setEditing(true)}
                className={cn(HEADER_BUTTON, "text-primary")}
              >
                <SlidersHorizontal className="size-3.5" />
              </Button>
              {canManage && (
                <Button
                  variant="ghost"
                  type="button"
                  aria-label={t`Edit scenes`}
                  onClick={() => setManaging(true)}
                  className={cn(HEADER_BUTTON, "text-primary")}
                >
                  <Pencil className="size-3.5" />
                </Button>
              )}
            </span>
          ))}
      </div>

      <NewSceneDialog
        path={path}
        open={creating}
        onClose={() => setCreating(false)}
        onCreated={refreshScenes}
      />
      <AlertDialog open={removing !== null} onOpenChange={(open) => !open && setRemoving(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              <Trans>Remove the scene {removing?.name}?</Trans>
            </AlertDialogTitle>
            <AlertDialogDescription>
              <Trans>
                It’s removed from OBS right away, with what’s on it: OBS deletes the sources no
                other scene uses, with their settings.
              </Trans>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>
              <Trans>Cancel</Trans>
            </AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => removing && remove.mutate(removing)}
            >
              <Trans>Remove</Trans>
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {managing && list.data && (
        <div className="flex flex-col gap-2">
          <p className="text-xs text-muted-foreground">
            <Trans>
              Changes here are made in OBS. The scene on air, and the scenes the switcher switches
              to, can’t be removed.
            </Trans>
          </p>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="self-start"
            onClick={() => setCreating(true)}
          >
            <Plus />
            <Trans>New scene</Trans>
          </Button>
          <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-3">
            {all.map((scene) => {
              const current = isCurrent(scene);
              const role = roleTags().find(([key]) => scene.uuid && scenes[key] === scene.uuid);
              const blocked = current
                ? t`It’s on air: switch to another scene first.`
                : role
                  ? t`The switcher uses it (${role[1]}): pick another on the Scenes page first.`
                  : null;
              return (
                <div
                  key={sceneKey(scene)}
                  className={cn(TILE, "relative border bg-card pr-10", current && "border-primary")}
                >
                  <span className="text-sm font-semibold">{scene.name}</span>
                  <span className="text-[11px] font-semibold tracking-wider text-muted-foreground">
                    {current ? t`ON AIR` : roleOf(scene.uuid)}
                  </span>
                  <Button
                    type="button"
                    size="icon"
                    variant="ghost"
                    className="absolute top-1.5 right-1.5 size-8 text-destructive hover:text-destructive"
                    aria-label={blocked ?? t`Remove the scene ${scene.name}`}
                    title={blocked ?? undefined}
                    disabled={!scene.uuid || blocked !== null || remove.isPending}
                    onClick={() =>
                      scene.uuid && setRemoving({ uuid: scene.uuid, name: scene.name })
                    }
                  >
                    <Trash2 />
                  </Button>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {editing && (
        <p className="text-xs text-muted-foreground">
          <Trans>
            Tap scenes to show or hide them here. Only this browser remembers it; the scene on air
            always shows.
          </Trans>
        </p>
      )}

      {!obsConnected && (
        <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
          <Trans>Scenes appear once OBS is connected.</Trans>
        </p>
      )}
      {obsConnected && list.isPending && <Skeleton className="h-40 rounded-2xl" />}
      {obsConnected && list.isError && (
        <p className="py-3 text-sm text-muted-foreground">
          <Trans>Couldn’t load scenes.</Trans>{" "}
          <Button
            variant="ghost"
            type="button"
            className="text-primary hover:underline"
            onClick={() => void list.refetch()}
          >
            <Trans>Retry</Trans>
          </Button>
        </p>
      )}

      {list.data && editing && (
        <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-3">
          {all.map((scene) => {
            const key = sceneKey(scene);
            const shown = !hiddenSet.has(key);
            return (
              <Button
                variant="ghost"
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
                    <span className="text-primary-soft-foreground">
                      <Trans>SHOWN</Trans>
                    </span>
                  ) : (
                    <>
                      <EyeOff className="size-3" aria-hidden="true" />
                      <Trans>HIDDEN</Trans>
                    </>
                  )}
                </span>
              </Button>
            );
          })}
        </div>
      )}

      {list.data &&
        !editing &&
        !managing &&
        visible.length > 0 &&
        onPreviewSelect &&
        (view === "grid" ? (
          <div
            data-scene-view={view}
            className="scene-bus"
            style={
              {
                "--scene-count": visible.length,
                "--bus-rows": Math.ceil(visible.length / 3),
              } as CSSProperties
            }
          >
            {(["PVW", "PGM"] as const).map((bus) => (
              <div className="bus-bank" key={bus}>
                <span
                  className={bus === "PVW" ? "bus-label preview-tally" : "bus-label program-tally"}
                >
                  {bus}
                </span>
                {visible.map((scene, index) => {
                  const active = scene.uuid === (bus === "PVW" ? previewUuid : currentUuid);
                  return (
                    <Button
                      variant="ghost"
                      type="button"
                      key={sceneKey(scene)}
                      disabled={
                        !canControl ||
                        !scene.uuid ||
                        !obsConnected ||
                        (bus === "PGM" && isCurrent(scene))
                      }
                      aria-label={`${bus === "PVW" ? "Cue" : "Cut to"} ${scene.name}`}
                      aria-pressed={active}
                      className={`bus-cell ${active ? (bus === "PVW" ? "lit-preview" : "lit-program") : ""}`}
                      onClick={() =>
                        scene.uuid &&
                        (bus === "PVW"
                          ? onPreviewSelect({ uuid: scene.uuid, name: scene.name })
                          : onProgramSelect?.({ uuid: scene.uuid, name: scene.name }))
                      }
                    >
                      <span className="bus-role">
                        {index < 9 && (
                          <kbd>
                            {bus === "PGM" ? "⇧" : ""}
                            {index + 1}
                          </kbd>
                        )}{" "}
                        {roleOf(scene.uuid)}
                      </span>
                      <strong>{scene.name}</strong>
                    </Button>
                  );
                })}
              </div>
            ))}
          </div>
        ) : (
          <div data-scene-view={view} className="scene-bus-list">
            {visible.map((scene, index) => (
              <div key={sceneKey(scene)}>
                <Button
                  variant="ghost"
                  type="button"
                  disabled={!canControl || !scene.uuid || !obsConnected}
                  className={scene.uuid === previewUuid ? "lit-preview" : ""}
                  aria-label={`Cue ${scene.name}`}
                  onClick={() =>
                    scene.uuid && onPreviewSelect({ uuid: scene.uuid, name: scene.name })
                  }
                >
                  <span className="preview-tally">PVW</span> <kbd>{index < 9 ? index + 1 : ""}</kbd>{" "}
                  <strong>{scene.name}</strong>
                  <small>{roleOf(scene.uuid)}</small>
                </Button>
                <Button
                  variant="ghost"
                  type="button"
                  disabled={!canControl || !scene.uuid || !obsConnected || isCurrent(scene)}
                  className={isCurrent(scene) ? "lit-program" : ""}
                  aria-label={`Cut to ${scene.name}`}
                  onClick={() =>
                    scene.uuid && onProgramSelect?.({ uuid: scene.uuid, name: scene.name })
                  }
                >
                  <span className="program-tally">PGM</span>
                  <kbd>{index < 9 ? `⇧${index + 1}` : ""}</kbd>
                </Button>
              </div>
            ))}
          </div>
        ))}
      {list.data && !editing && !managing && visible.length > 0 && !onPreviewSelect && (
        <div
          data-scene-view={view}
          style={{ "--scene-count": visible.length } as CSSProperties}
          className={cn(
            "scene-cells grid overflow-hidden bg-divider",
            view === "grid" ? "grid-cols-3" : "grid-cols-1",
          )}
        >
          {visible.map((scene, index) => {
            const current = isCurrent(scene);
            const pending = switchScene.isPending && switchScene.variables === scene.uuid;
            return (
              <Button
                variant="ghost"
                key={sceneKey(scene)}
                type="button"
                // Only the scene being switched to waits: on a slow connection the
                // other tiles stay usable, and the switches go out in tap order.
                disabled={!canControl || !scene.uuid || (!onPreviewSelect && current) || pending}
                aria-pressed={current}
                onClick={() =>
                  scene.uuid &&
                  (onProgramSelect
                    ? onProgramSelect({ uuid: scene.uuid, name: scene.name })
                    : switchScene.mutate(scene.uuid))
                }
                className={cn(
                  "scene-cell relative flex min-w-0 flex-col items-start justify-between gap-3 bg-card px-4 py-3 text-left transition-colors disabled:cursor-default enabled:hover:bg-surface-2",
                  current && "scene-on-air",
                  pending && "animate-pulse",
                  scene.uuid === previewUuid &&
                    "outline outline-dashed -outline-offset-2 outline-primary",
                )}
              >
                <span className="scene-copy flex min-w-0 flex-col">
                  <span className="scene-name text-sm font-semibold">
                    {index < 9 && <kbd className="scene-key">{index + 1}</kbd>}
                    {scene.name}
                  </span>
                  {view === "list" && (
                    <span className="scene-role text-muted-foreground">{roleOf(scene.uuid)}</span>
                  )}
                </span>
                <span className="scene-secondary text-xs text-muted-foreground">
                  {current && (
                    <span className="inline-flex items-center gap-1.5 rounded-full bg-offline/10 px-2 py-0.5 text-[10px] leading-4 font-medium text-offline">
                      <span className="size-1.5 rounded-full bg-offline" aria-hidden="true" />
                      <Trans>On air</Trans>
                    </span>
                  )}
                  {!current && scene.uuid === previewUuid && (
                    <span className="inline-flex rounded-full bg-primary-soft px-2 py-0.5 text-[10px] leading-4 font-medium text-primary">
                      <Trans>PREVIEW</Trans>
                    </span>
                  )}
                  {view === "grid" && !current && scene.uuid !== previewUuid && roleOf(scene.uuid)}
                </span>
                {current && <span className="absolute inset-x-0 bottom-0 h-0.5 bg-offline" />}
              </Button>
            );
          })}
        </div>
      )}

      {list.data && !editing && !managing && visible.length === 0 && (
        <Button
          variant="ghost"
          type="button"
          onClick={() => setEditing(true)}
          className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground hover:bg-surface-2"
        >
          <Trans>All scenes are hidden. Tap to pick some.</Trans>
        </Button>
      )}
    </section>
  );
}

/** A name for a new, empty scene. */
function NewSceneDialog({
  path,
  open,
  onClose,
  onCreated,
}: {
  path: { workspace_id: string; switcher_id: string };
  open: boolean;
  onClose: () => void;
  onCreated: () => void;
}) {
  const id = useId();
  const [name, setName] = useState("");
  const create = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scenes", {
          params: { path },
          body: { name },
        }),
      ),
    onSuccess: (created) => {
      toast.success(t`Added the scene ${created.name}`);
      setName("");
      onCreated();
      onClose();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  const submit = (event: FormEvent) => {
    event.preventDefault();
    create.mutate();
  };
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="sm:max-w-sm">
        <form onSubmit={submit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>
              <Trans>New scene</Trans>
            </DialogTitle>
            <DialogDescription>
              <Trans>An empty scene in OBS. Add sources to it from the Sources card.</Trans>
            </DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor={id}>
              <Trans>Name</Trans>
            </Label>
            <Input
              id={id}
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={t`Intermission`}
              autoComplete="off"
              required
            />
          </div>
          <DialogFooter>
            <Button type="submit" disabled={create.isPending}>
              <Trans>Add scene</Trans>
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
