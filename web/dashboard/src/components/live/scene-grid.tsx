import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { EyeOff, LayoutGrid, List, Pencil, Plus, Trash2 } from "lucide-react";
import { type FormEvent, type ReactNode, useId, useState } from "react";
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
import { PaneTitleRow } from "./pane-title-row";

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
  lastKnownUuid,
  streaming,
  canControl,
  canManage,
  obsConnected,
  previewUuid,
  onPreviewSelect,
  onProgramSelect,
  operatorTools,
  hotkeys = false,
}: {
  path: { workspace_id: string; switcher_id: string };
  scenes: Scenes;
  currentUuid: string | null | undefined;
  lastKnownUuid?: string;
  streaming: boolean;
  canControl: boolean;
  /** May add and remove scenes in OBS (members who manage switchers, not while paused). */
  canManage: boolean;
  obsConnected: boolean;
  previewUuid?: string;
  onPreviewSelect?: (scene: { uuid: string; name: string }) => void;
  onProgramSelect?: (scene: { uuid: string; name: string }) => void;
  operatorTools?: ReactNode;
  hotkeys?: boolean;
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
  // One Edit state: who may change OBS also gets the list that adds, renames and removes.
  const managing = editing && canManage;
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
    scene.uuid != null && scene.uuid === (obsConnected ? currentUuid : lastKnownUuid);
  const currentLabel = !obsConnected ? t`Last known` : streaming ? t`On air` : t`Active`;
  // The scene on air always shows, hidden or not.
  const visible = all.filter((scene) => !hiddenSet.has(sceneKey(scene)) || isCurrent(scene));
  const hiddenCount = all.length - visible.length;
  const anyHidden = all.some((scene) => hiddenSet.has(sceneKey(scene)));

  const [search, setSearch] = useState("");
  const large = all.length > 8;
  // The search field only shows for a long list; a filter left from one is dropped.
  const shown = visible.filter(
    (scene) => !large || scene.name.toLowerCase().includes(search.trim().toLowerCase()),
  );
  /** Number hints show where the hotkeys work: operator mode. */
  const keys = hotkeys;
  const selections = large
    ? all.filter((scene) => isCurrent(scene) || (onPreviewSelect && scene.uuid === previewUuid))
    : [];

  const toggle = (key: string) =>
    setHidden(hiddenSet.has(key) ? hidden.filter((item) => item !== key) : [...hidden, key]);

  return (
    <section aria-label={t`Scenes`} className="@container flex flex-col gap-3.5">
      <PaneTitleRow className="justify-between">
        <h2 className="text-[15px] font-semibold tracking-tight">
          <Trans>Scenes</Trans>
        </h2>
        {!obsConnected && list.data && (
          <span className="min-w-0 flex-1 truncate text-xs text-low">
            <Trans>OBS disconnected · last known scenes</Trans>
          </span>
        )}
        {list.data &&
          (editing ? (
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
              <span className="mr-1 hidden @[34rem]:inline">
                {hiddenCount > 0
                  ? t`${hiddenCount} hidden`
                  : canControl
                    ? onPreviewSelect
                      ? t`Click to cue · Shift+click cuts`
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
                aria-label={t`Edit scenes`}
                onClick={() => setEditing(true)}
                className={cn(HEADER_BUTTON, "text-primary")}
              >
                <Pencil className="size-3.5" />
              </Button>
            </span>
          ))}
      </PaneTitleRow>

      {large && !editing && (
        <div className="flex flex-col gap-2">
          <Input
            data-scene-search
            type="search"
            aria-label={t`Search scenes`}
            placeholder={keys ? t`Search scenes · /` : t`Search scenes`}
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            className="h-9"
          />
          {keys && (
            <p className="text-xs text-muted-foreground">
              <Trans>1–9: first nine scenes · /: search</Trans>
            </p>
          )}
          <fieldset
            aria-label={t`Scene selections`}
            className={cn("flex flex-col gap-1 text-xs", !obsConnected && "opacity-50")}
          >
            {selections.map((scene) => (
              <span key={sceneKey(scene)} className="flex min-w-0 items-center gap-2">
                <span className="shrink-0 font-semibold">
                  {isCurrent(scene)
                    ? currentLabel
                    : obsConnected
                      ? t`Preview`
                      : t`Preview · last known`}
                </span>
                <span className="min-w-0 truncate" title={scene.name}>
                  {scene.name}
                </span>
              </span>
            ))}
          </fieldset>
        </div>
      )}

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
          <div
            className={cn(
              "grid content-start grid-cols-2 gap-2.5 sm:grid-cols-3",
              large && "live-collection-scroll",
            )}
          >
            {all.map((scene) => {
              const current = isCurrent(scene);
              const role = roleTags().find(([key]) => scene.uuid && scenes[key] === scene.uuid);
              const blocked = current
                ? t`It’s the current scene: switch to another scene first.`
                : role
                  ? t`The switcher uses it (${role[1]}): pick another on the Scenes page first.`
                  : null;
              return (
                <div
                  key={sceneKey(scene)}
                  className={cn(TILE, "relative border bg-card pr-10", current && "border-primary")}
                >
                  <span className="w-full truncate text-sm font-semibold" title={scene.name}>
                    {scene.name}
                  </span>
                  <span className="text-[11px] font-semibold tracking-wider text-muted-foreground">
                    {current ? currentLabel : roleOf(scene.uuid)}
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
            Tap scenes below to show or hide them here. Only this browser remembers it; the scene on
            air always shows.
          </Trans>
        </p>
      )}

      {!obsConnected && !list.data && (
        <p className="text-sm text-low">
          <Trans>OBS disconnected · no current scene data</Trans>
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
        <div
          className={cn(
            "grid content-start grid-cols-2 gap-2.5 sm:grid-cols-3",
            large && "live-collection-scroll",
          )}
        >
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
                <span className="w-full truncate text-sm font-semibold" title={scene.name}>
                  {scene.name}
                </span>
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

      {list.data && !editing && visible.length > 0 && (
        <div
          data-scene-view={view}
          className={cn(
            "grid content-start",
            view === "grid" ? "gap-2.5" : "gap-1",
            large && "live-collection-scroll",
            view === "grid" ? "grid-cols-[repeat(auto-fill,minmax(140px,1fr))]" : "grid-cols-1",
            !obsConnected && "opacity-50",
          )}
        >
          {shown.map((scene) => {
            const index = visible.indexOf(scene);
            const current = isCurrent(scene);
            const cued = Boolean(onPreviewSelect) && scene.uuid === previewUuid && !current;
            const pending = switchScene.isPending && switchScene.variables === scene.uuid;
            const pick = { uuid: scene.uuid ?? "", name: scene.name };
            return (
              <button
                key={sceneKey(scene)}
                type="button"
                // Only the scene being switched to waits: on a slow connection the
                // other tiles stay usable, and the switches go out in tap order.
                disabled={
                  !canControl ||
                  !scene.uuid ||
                  !obsConnected ||
                  (!onPreviewSelect && current) ||
                  pending
                }
                aria-pressed={onPreviewSelect ? cued : current}
                aria-label={onPreviewSelect ? t`Cue ${scene.name}` : undefined}
                onClick={(event) =>
                  onPreviewSelect && !event.shiftKey
                    ? onPreviewSelect(pick)
                    : onProgramSelect
                      ? onProgramSelect(pick)
                      : scene.uuid && switchScene.mutate(scene.uuid)
                }
                className={cn(
                  "relative flex min-w-0 rounded-xl border bg-muted text-left transition-colors enabled:hover:border-border-strong enabled:hover:bg-surface-2",
                  view === "grid"
                    ? "min-h-[76px] flex-col justify-between gap-2 px-3.5 py-3"
                    : "min-h-11 items-center gap-3 px-3.5 py-1 md:min-h-9",
                  current &&
                    (obsConnected && streaming
                      ? "border-offline/60 bg-offline/10"
                      : "border-border-strong bg-surface-2"),
                  cued && obsConnected && "border-live/60 bg-live/10",
                  pending && "animate-pulse",
                )}
              >
                <span
                  className={cn(
                    "flex min-w-0 items-center gap-2 text-[11px] text-subtle-foreground",
                    view === "grid"
                      ? "w-full justify-between"
                      : "order-last ml-auto max-w-24 shrink-0",
                  )}
                >
                  <span className="truncate">
                    {current ? (
                      <span
                        className={cn(
                          "font-semibold",
                          obsConnected && streaming ? "text-offline" : "text-muted-foreground",
                        )}
                      >
                        {currentLabel}
                      </span>
                    ) : cued ? (
                      <span className="font-semibold text-live">
                        {obsConnected ? t`Preview` : t`Preview · last known`}
                      </span>
                    ) : (
                      roleOf(scene.uuid)
                    )}
                  </span>
                  {keys && index < 9 && (
                    <kbd className="rounded-[5px] bg-surface-2 px-1.5 py-px font-mono text-[10px] text-subtle-foreground">
                      {index + 1}
                    </kbd>
                  )}
                </span>
                <span className="min-w-0 truncate text-sm font-semibold" title={scene.name}>
                  {scene.name}
                </span>
              </button>
            );
          })}
          {shown.length === 0 && (
            <p className="py-3 text-sm text-muted-foreground">
              <Trans>No scene matches “{search}”.</Trans>
            </p>
          )}
        </div>
      )}

      {list.data && !editing && visible.length === 0 && (
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
