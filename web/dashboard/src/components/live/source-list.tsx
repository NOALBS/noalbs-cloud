import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Box,
  Camera,
  Clapperboard,
  Eye,
  EyeOff,
  Folder,
  Globe,
  Image,
  Layers,
  type LucideIcon,
  Monitor,
  SlidersHorizontal,
  Type,
  Volume2,
} from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { obsSourcesQuery, queryKeys } from "@/api/queries";
import type { ObsSource } from "@/api/types";
import { Skeleton } from "@/components/ui/skeleton";
import { errorMessage } from "@/lib/errors";
import { sceneKey, useHiddenSources } from "@/lib/hidden-scenes";
import {
  keptSections,
  type SourceKind,
  sourceKey,
  sourceKind,
  sourceSections,
  withEnabled,
} from "@/lib/obs-sources";
import { cn } from "@/lib/utils";

const ICONS: Record<SourceKind, LucideIcon> = {
  scene: Layers,
  group: Folder,
  media: Clapperboard,
  camera: Camera,
  capture: Monitor,
  audio: Volume2,
  browser: Globe,
  text: Type,
  image: Image,
  other: Box,
};

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
 */
export function SourceList({
  path,
  scene,
  canControl,
  obsConnected,
}: {
  path: { workspace_id: string; switcher_id: string };
  /** The scene on air (UUID, else name), so a switch loads the new scene's sources. */
  scene: string | null;
  canControl: boolean;
  obsConnected: boolean;
}) {
  const [hidden, setHidden] = useHiddenSources(path.switcher_id);
  const [editing, setEditing] = useState(false);
  const queryClient = useQueryClient();
  const query = obsSourcesQuery(path.workspace_id, path.switcher_id, scene);
  const list = useQuery({ ...query, enabled: obsConnected, refetchInterval: REFRESH_MS });

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
    <section aria-label="Sources" className="flex flex-col gap-2.5">
      <div className="flex min-h-8 items-center justify-between gap-2">
        <h2 className="font-semibold">Sources</h2>
        {list.data &&
          onScene.length > 0 &&
          (editing ? (
            <span className="flex items-center gap-1">
              <button
                type="button"
                onClick={() => setAllHere(hiddenHere === 0)}
                className={cn(HEADER_BUTTON, "text-muted-foreground hover:text-foreground")}
              >
                {hiddenHere > 0 ? "Show all" : "Hide all"}
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
              {hiddenHere > 0 && `${hiddenHere} hidden`}
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
          Tap sources to show or hide them here. Only this browser remembers it, and OBS isn’t
          changed.
        </p>
      )}

      {!obsConnected && (
        <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
          Sources appear once OBS is connected.
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
          This scene has no sources.
        </p>
      )}

      {list.data && !editing && sections.length > 0 && listedSections.length === 0 && (
        <button
          type="button"
          onClick={() => setEditing(true)}
          className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground hover:bg-surface-2"
        >
          All sources are hidden. Tap to pick some.
        </button>
      )}

      {(editing ? sections : listedSections).map((section) => (
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
              <span className="truncate">In {section.parent}</span>
              {!section.parentShown && <span className="font-normal normal-case">· hidden</span>}
            </h3>
          )}
          <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-3">
            {section.items.map(({ source, shown }) => {
              const key = sourceKey(source);
              const Icon = ICONS[sourceKind(source)];
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
                    {source.enabled ? (shown ? "SHOWN" : "SHOWN · PARENT HIDDEN") : "HIDDEN"}
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      ))}
    </section>
  );
}
