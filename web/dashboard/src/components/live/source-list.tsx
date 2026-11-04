import {
  closestCenter,
  DndContext,
  type DragEndEvent,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Copy,
  Ellipsis,
  Eye,
  EyeOff,
  Folder,
  GripVertical,
  Layers,
  Move,
  Pencil,
  Plus,
  SlidersHorizontal,
  Trash2,
} from "lucide-react";
import { type ReactNode, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { obsSourcesQuery, queryKeys } from "@/api/queries";
import type { ObsSource } from "@/api/types";
import { type EditorTarget, SourceEditorDialog } from "@/components/live/source-editor";
import { SOURCE_ICONS } from "@/components/live/source-icons";
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
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { errorMessage } from "@/lib/errors";
import { sceneKey, useHiddenSources } from "@/lib/hidden-scenes";
import {
  duplicable,
  editableKind,
  keptSections,
  removable,
  sourceKey,
  sourceKind,
  sourceSections,
  withEnabled,
  withMovedTo,
  withRemoved,
} from "@/lib/obs-sources";
import { cn } from "@/lib/utils";

/** Like the scene tiles, so both grids read the same. */
const TILE =
  "flex min-h-19 flex-col items-start justify-between gap-1.5 rounded-xl p-3 text-left transition-colors";

const HEADER_BUTTON =
  "flex h-8 items-center gap-1.5 rounded-lg px-2 text-xs font-medium hover:bg-surface-2";

/** How often the list is re-read, so changes made in OBS show up. */
const REFRESH_MS = 10_000;

/**
 * The sources on the scene on air as tiles like the scenes: tap one to show or
 * hide it (like `!source`). Sources inside nested scenes and groups get a
 * labelled grid of their own under the scene's. "Customize" hides sources
 * from the card for this viewer (by source, so on every scene), like the scene grid.
 *
 * "Edit" changes OBS itself: sources move up and down their scene or group,
 * and media, VLC, browser and text sources can be added and edited. Moving is only
 * possible there, so a tap on a phone can't rearrange the scene by accident.
 */
export function SourceList({
  path,
  scene,
  canControl,
  canEditSources,
  obsConnected,
}: {
  path: { workspace_id: string; switcher_id: string };
  /** The scene on air (UUID, else name), so a switch loads the new scene's sources. */
  scene: string | null;
  canControl: boolean;
  /** May add and edit sources (members who manage switchers, not while paused). */
  canEditSources: boolean;
  obsConnected: boolean;
}) {
  const [hidden, setHidden] = useHiddenSources(path.switcher_id);
  const [editing, setEditing] = useState(false);
  const [arranging, setArranging] = useState(false);
  const [editor, setEditor] = useState<EditorTarget | null>(null);
  const queryClient = useQueryClient();
  const query = obsSourcesQuery(path.workspace_id, path.switcher_id, scene);
  const list = useQuery({ ...query, enabled: obsConnected, refetchInterval: REFRESH_MS });

  const move = useMutation({
    mutationFn: ({ source, position }: { source: ObsSource; position: number }) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/source/move", {
          params: { path },
          body: {
            parent: source.parent,
            parent_is_group: source.parent_is_group,
            item_id: source.item_id,
            position,
          },
        }),
      ),
    onMutate: async ({ source, position }) => {
      await queryClient.cancelQueries({ queryKey: query.queryKey });
      const previous = queryClient.getQueryData(query.queryKey);
      queryClient.setQueryData(
        query.queryKey,
        (data) => data && withMovedTo(data, sourceKey(source), position),
      );
      return { previous };
    },
    onError: (error, _, context) => {
      if (context?.previous) queryClient.setQueryData(query.queryKey, context.previous);
      toast.error(errorMessage(error));
    },
    onSettled: () =>
      queryClient.invalidateQueries({
        queryKey: queryKeys.obsSources(path.workspace_id, path.switcher_id),
      }),
  });
  const [removing, setRemoving] = useState<ObsSource | null>(null);
  const remove = useMutation({
    mutationFn: (source: ObsSource) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/source/remove", {
          params: { path },
          body: {
            parent: source.parent,
            parent_is_group: source.parent_is_group,
            item_id: source.item_id,
          },
        }),
      ),
    onMutate: async (source) => {
      setRemoving(null);
      await queryClient.cancelQueries({ queryKey: query.queryKey });
      const previous = queryClient.getQueryData(query.queryKey);
      queryClient.setQueryData(
        query.queryKey,
        (data) => data && withRemoved(data, sourceKey(source)),
      );
      return { previous };
    },
    onSuccess: (_, source) => toast.success(t`Removed ${source.name} from ${source.parent}`),
    onError: (error, _, context) => {
      if (context?.previous) queryClient.setQueryData(query.queryKey, context.previous);
      toast.error(errorMessage(error));
    },
    onSettled: () => {
      void queryClient.invalidateQueries({
        queryKey: queryKeys.obsSources(path.workspace_id, path.switcher_id),
      });
      void queryClient.invalidateQueries({
        queryKey: queryKeys.obsAudio(path.workspace_id, path.switcher_id),
      });
    },
  });
  const canArrange = canControl || canEditSources;

  // Like the servers page: a short drag starts it, and the keyboard works too.
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );
  const dropped = (items: { source: ObsSource }[], { active, over }: DragEndEvent) => {
    if (!over || active.id === over.id) return;
    const source = items.find((item) => sourceKey(item.source) === active.id)?.source;
    const position = items.findIndex((item) => sourceKey(item.source) === over.id);
    if (source && position >= 0) move.mutate({ source, position });
  };

  const toggle = useMutation({
    mutationFn: ({ source, enabled }: { source: ObsSource; enabled: boolean }) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/source", {
          params: { path },
          body: {
            parent: source.parent,
            parent_is_group: source.parent_is_group,
            item_id: source.item_id,
            enabled,
          },
        }),
      ),
    onMutate: async ({ source, enabled }) => {
      await queryClient.cancelQueries({ queryKey: query.queryKey });
      const previous = queryClient.getQueryData(query.queryKey);
      queryClient.setQueryData(
        query.queryKey,
        (data) => data && withEnabled(data, sourceKey(source), enabled),
      );
      return { previous };
    },
    onError: (error, _, context) => {
      if (context?.previous) queryClient.setQueryData(query.queryKey, context.previous);
      toast.error(errorMessage(error));
    },
    onSettled: () =>
      queryClient.invalidateQueries({
        queryKey: queryKeys.obsSources(path.workspace_id, path.switcher_id),
      }),
  });

  const sections = sourceSections(list.data?.sources ?? []);
  const hiddenSet = new Set(hidden);
  const listedSections = keptSections(sections, hiddenSet);
  const onScene = [
    ...new Set(sections.flatMap((section) => section.items.map(({ source }) => sceneKey(source)))),
  ];
  const hiddenHere = onScene.filter((key) => hiddenSet.has(key)).length;

  const toggleHidden = (key: string) =>
    setHidden(hiddenSet.has(key) ? hidden.filter((item) => item !== key) : [...hidden, key]);
  // Only this scene's sources: what was hidden on other scenes stays hidden.
  const setAllHere = (hide: boolean) =>
    setHidden(
      hide ? [...new Set([...hidden, ...onScene])] : hidden.filter((key) => !onScene.includes(key)),
    );

  return (
    <section aria-label={t`Sources`} className="flex flex-col gap-2.5">
      <div className="flex min-h-8 items-center justify-between gap-2">
        <h2 className="font-semibold">
          <Trans>Sources</Trans>
        </h2>
        {list.data &&
          (onScene.length > 0 || canEditSources) &&
          (arranging ? (
            <button
              type="button"
              onClick={() => setArranging(false)}
              className={cn(HEADER_BUTTON, "bg-primary text-primary-foreground hover:bg-primary")}
            >
              <Trans>Done</Trans>
            </button>
          ) : editing ? (
            <span className="flex items-center gap-1">
              <button
                type="button"
                onClick={() => setAllHere(hiddenHere === 0)}
                className={cn(HEADER_BUTTON, "text-muted-foreground hover:text-foreground")}
              >
                {hiddenHere > 0 ? t`Show all` : t`Hide all`}
              </button>
              <button
                type="button"
                onClick={() => setEditing(false)}
                className={cn(HEADER_BUTTON, "bg-primary text-primary-foreground hover:bg-primary")}
              >
                <Trans>Done</Trans>
              </button>
            </span>
          ) : (
            <span className="flex items-center gap-1 text-xs text-muted-foreground">
              {hiddenHere > 0 && t`${hiddenHere} hidden`}
              {onScene.length > 0 && (
                <button
                  type="button"
                  onClick={() => setEditing(true)}
                  className={cn(HEADER_BUTTON, "text-primary")}
                >
                  <SlidersHorizontal className="size-3.5" />
                  <Trans>Customize</Trans>
                </button>
              )}
              {canArrange && (
                <button
                  type="button"
                  onClick={() => setArranging(true)}
                  className={cn(HEADER_BUTTON, "text-primary")}
                >
                  <Pencil className="size-3.5" />
                  <Trans>Edit</Trans>
                </button>
              )}
            </span>
          ))}
      </div>

      <SourceEditorDialog path={path} target={editor} onClose={() => setEditor(null)} />
      <AlertDialog open={removing !== null} onOpenChange={(open) => !open && setRemoving(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              <Trans>
                Remove {removing?.name} from {removing?.parent}?
              </Trans>
            </AlertDialogTitle>
            <AlertDialogDescription>
              {removing?.is_scene ? (
                <Trans>
                  It’s removed from this scene in OBS right away, on stream. The scene itself and
                  what’s in it stay.
                </Trans>
              ) : (
                <Trans>
                  It’s removed in OBS right away, on stream. If no other scene uses it, OBS deletes
                  it with its settings.
                </Trans>
              )}
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

      {arranging && list.data && (
        <div className="flex flex-col gap-2">
          <p className="text-xs text-muted-foreground">
            {canEditSources ? (
              <Trans>
                Changes here are made in OBS, on stream. Move sources up to show them over the
                others, or edit media, VLC, browser and text sources.
              </Trans>
            ) : (
              <Trans>
                Changes here are made in OBS, on stream. Move sources up to show them over the
                others.
              </Trans>
            )}
          </p>
          {canEditSources && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="self-start"
              onClick={() => setEditor({ mode: "create", scene: list.data.scene })}
            >
              <Plus />
              <Trans>Add source</Trans>
            </Button>
          )}
        </div>
      )}

      {editing && (
        <p className="text-xs text-muted-foreground">
          <Trans>
            Tap sources to show or hide them here. Only this browser remembers it, and OBS isn’t
            changed.
          </Trans>
        </p>
      )}

      {!obsConnected && (
        <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
          <Trans>Sources appear once OBS is connected.</Trans>
        </p>
      )}
      {obsConnected && list.isPending && <Skeleton className="h-32 rounded-2xl" />}
      {obsConnected && list.isError && !list.data && (
        <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
          {errorMessage(list.error)}
        </p>
      )}

      {list.data && sections.length === 0 && (
        <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
          <Trans>This scene has no sources.</Trans>
        </p>
      )}

      {list.data &&
        !editing &&
        !arranging &&
        sections.length > 0 &&
        listedSections.length === 0 && (
          <button
            type="button"
            onClick={() => setEditing(true)}
            className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground hover:bg-surface-2"
          >
            <Trans>All sources are hidden. Tap to pick some.</Trans>
          </button>
        )}

      {(editing || arranging ? sections : listedSections).map((section) => (
        <div
          key={section.parent === null ? "root" : `${section.parentIsGroup}:${section.parent}`}
          className="flex flex-col gap-2"
        >
          {section.parent !== null && (
            <h3 className="mt-1 flex items-center gap-1.5 text-xs font-semibold tracking-wider text-muted-foreground uppercase">
              {section.parentIsGroup ? (
                <Folder className="size-3.5" aria-hidden="true" />
              ) : (
                <Layers className="size-3.5" aria-hidden="true" />
              )}
              <span className="truncate">
                <Trans>In {section.parent}</Trans>
              </span>
              {!section.parentShown && (
                <span className="font-normal normal-case">
                  <Trans>· hidden</Trans>
                </span>
              )}
            </h3>
          )}
          {arranging ? (
            // Each scene or group sorts on its own: OBS moves items only within theirs.
            <DndContext
              sensors={sensors}
              collisionDetection={closestCenter}
              onDragEnd={(event) => dropped(section.items, event)}
            >
              <SortableContext
                items={section.items.map(({ source }) => sourceKey(source))}
                strategy={verticalListSortingStrategy}
              >
                <ul className="flex flex-col divide-y divide-divider rounded-xl border">
                  {section.items.map(({ source }) => {
                    const editable = canEditSources && editableKind(source) !== null;
                    const canRemove = canEditSources && removable(source);
                    const canDuplicate = canEditSources && duplicable(source);
                    return (
                      <SortableSourceRow
                        key={sourceKey(source)}
                        source={source}
                        canMove={canControl}
                        menu={
                          (editable || canControl || canDuplicate || canRemove) && (
                            <DropdownMenu>
                              <DropdownMenuTrigger asChild>
                                <Button
                                  type="button"
                                  size="icon"
                                  variant="ghost"
                                  className="size-8"
                                  aria-label={t`More for ${source.name}`}
                                >
                                  <Ellipsis />
                                </Button>
                              </DropdownMenuTrigger>
                              <DropdownMenuContent align="end">
                                {editable && (
                                  <DropdownMenuItem
                                    onSelect={() => setEditor({ mode: "edit", name: source.name })}
                                  >
                                    <Pencil />
                                    <Trans>Edit…</Trans>
                                  </DropdownMenuItem>
                                )}
                                {canControl && (
                                  <DropdownMenuItem
                                    onSelect={() => setEditor({ mode: "position", source })}
                                  >
                                    <Move />
                                    <Trans>Position…</Trans>
                                  </DropdownMenuItem>
                                )}
                                {canDuplicate && (
                                  <DropdownMenuItem
                                    onSelect={() =>
                                      setEditor({
                                        mode: "duplicate",
                                        source,
                                        // OBS can't add to a group: then the scene on air.
                                        scene: source.parent_is_group
                                          ? (list.data?.scene ?? source.parent)
                                          : source.parent,
                                        taken: (list.data?.sources ?? []).map((item) => item.name),
                                      })
                                    }
                                  >
                                    <Copy />
                                    <Trans>Duplicate…</Trans>
                                  </DropdownMenuItem>
                                )}
                                {(editable || canControl || canDuplicate) && canRemove && (
                                  <DropdownMenuSeparator />
                                )}
                                {canRemove && (
                                  <DropdownMenuItem
                                    variant="destructive"
                                    onSelect={() => setRemoving(source)}
                                  >
                                    <Trash2 />
                                    <Trans>Remove from {source.parent}…</Trans>
                                  </DropdownMenuItem>
                                )}
                              </DropdownMenuContent>
                            </DropdownMenu>
                          )
                        }
                      />
                    );
                  })}
                </ul>
              </SortableContext>
            </DndContext>
          ) : (
            <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-3">
              {section.items.map(({ source, shown }) => {
                const key = sourceKey(source);
                const Icon = SOURCE_ICONS[sourceKind(source)];
                const name = (
                  <span className="flex w-full min-w-0 items-center gap-2">
                    <Icon className="size-4 shrink-0" aria-hidden="true" />
                    <span className="truncate text-sm font-semibold">{source.name}</span>
                  </span>
                );

                if (editing) {
                  const listed = !hiddenSet.has(sceneKey(source));
                  return (
                    <button
                      key={key}
                      type="button"
                      aria-pressed={listed}
                      onClick={() => toggleHidden(sceneKey(source))}
                      className={cn(
                        TILE,
                        listed
                          ? "border-2 border-primary bg-primary-soft"
                          : "border border-dashed border-border-strong bg-transparent text-muted-foreground opacity-70",
                      )}
                    >
                      {name}
                      <span className="flex items-center gap-1.5 text-[11px] font-semibold tracking-wider">
                        {listed ? (
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
                    </button>
                  );
                }

                const pending = toggle.isPending && sourceKey(toggle.variables.source) === key;
                return (
                  <button
                    key={key}
                    type="button"
                    aria-pressed={source.enabled}
                    disabled={!canControl || pending}
                    onClick={() => toggle.mutate({ source, enabled: !source.enabled })}
                    className={cn(
                      TILE,
                      "disabled:cursor-default",
                      source.enabled
                        ? "border-2 border-primary bg-primary-soft"
                        : "border border-dashed border-border-strong bg-transparent text-muted-foreground",
                      source.enabled && !shown && "opacity-60",
                      pending && "animate-pulse",
                    )}
                  >
                    {name}
                    <span
                      className={cn(
                        "flex items-center gap-1.5 text-[11px] font-semibold tracking-wider",
                        source.enabled && "text-primary-soft-foreground",
                      )}
                    >
                      {source.enabled ? (
                        <Eye className="size-3" aria-hidden="true" />
                      ) : (
                        <EyeOff className="size-3" aria-hidden="true" />
                      )}
                      {source.enabled ? (shown ? t`SHOWN` : t`SHOWN · PARENT HIDDEN`) : t`HIDDEN`}
                    </span>
                  </button>
                );
              })}
            </div>
          )}
        </div>
      ))}
    </section>
  );
}

/** A row of the Edit list: dragged by its grip (who may move sources), with its ⋯ menu. */
function SortableSourceRow({
  source,
  canMove,
  menu,
}: {
  source: ObsSource;
  canMove: boolean;
  menu: ReactNode;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: sourceKey(source), disabled: !canMove });
  const Icon = SOURCE_ICONS[sourceKind(source)];
  return (
    <li
      ref={setNodeRef}
      style={{
        transform: transform ? `translate3d(0, ${transform.y}px, 0)` : undefined,
        transition,
      }}
      className={cn(
        "flex min-h-11 items-center gap-2 bg-card py-1 pr-3 first:rounded-t-xl last:rounded-b-xl",
        canMove ? "pl-1" : "pl-3",
        isDragging && "relative z-10 rounded-xl border border-primary shadow-2xl",
      )}
    >
      {canMove && (
        <button
          type="button"
          ref={setActivatorNodeRef}
          {...attributes}
          {...listeners}
          aria-label={t`Move ${source.name}`}
          className="flex size-8 shrink-0 cursor-grab touch-none items-center justify-center rounded-lg text-subtle-foreground hover:text-foreground active:cursor-grabbing"
        >
          <GripVertical className="size-4" />
        </button>
      )}
      <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
      <span
        className={cn(
          "min-w-0 flex-1 truncate text-sm font-semibold",
          !source.enabled && "text-muted-foreground",
        )}
      >
        {source.name}
      </span>
      {menu}
    </li>
  );
}
