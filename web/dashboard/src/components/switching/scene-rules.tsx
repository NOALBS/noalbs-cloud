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
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { plural, t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, GripVertical, Pencil, Plus, Trash2, TriangleAlert } from "lucide-react";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys, sceneRulesQuery } from "@/api/queries";
import type { ObsScenes, SceneRule, StreamServer } from "@/api/types";
import { SceneChoice as ScenePicker } from "@/components/switching/scene-choice";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
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
import { Switch } from "@/components/ui/switch";
import { useSwitcher } from "@/hooks/use-switcher";
import { errorMessage } from "@/lib/errors";
import { SCENE_ROLES } from "@/lib/scene-roles";
import { disabledServers, findDuplicate, ruleLabel, shadowedBy } from "@/lib/scene-rules";
import { cn } from "@/lib/utils";
import { isStale } from "@/live/reducer";
import { useSwitcherLive } from "@/live/use-switcher-live";

/** Mirrors the API's limit. */
const MAX_RULES = 20;
const ROLES = ["live", "low", "offline"] as const;

type Scenes = SceneRule["scenes"];
type Draft = { server_ids: string[]; scenes: Scenes };

/** Scenes to use while several servers are live at once, tried in order. */
export function SceneRulesSection({
  obs,
  servers,
}: {
  obs: ObsScenes | undefined;
  servers: StreamServer[];
}) {
  const { workspace, switcher, canManage, path } = useSwitcher();
  const queryClient = useQueryClient();
  const key = queryKeys.sceneRules(workspace.id, switcher.id);
  const rules = useQuery(sceneRulesQuery(workspace.id, switcher.id));
  const live = useSwitcherLive(workspace.id, switcher.id);
  const status = live.status && !isStale(live.status) ? live.status : null;
  // `null`: closed; `undefined` rule: adding.
  const [editing, setEditing] = useState<{ rule?: SceneRule } | null>(null);

  const list = rules.data ?? [];
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const reorder = useMutation({
    mutationFn: (ids: string[]) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/scene-rules/order", {
          params: { path },
          body: { ids },
        }),
      ),
    onMutate: async (ids) => {
      await queryClient.cancelQueries({ queryKey: key });
      const previous = queryClient.getQueryData<SceneRule[]>(key);
      queryClient.setQueryData<SceneRule[]>(key, (current) =>
        ids
          .map((id) => current?.find((rule) => rule.id === id))
          .filter((rule): rule is SceneRule => rule !== undefined),
      );
      return { previous };
    },
    onError: (error, _, context) => {
      queryClient.setQueryData(key, context?.previous);
      toast.error(errorMessage(error));
    },
    onSuccess: (ordered) => queryClient.setQueryData(key, ordered),
  });

  const onDragEnd = ({ active, over }: DragEndEvent) => {
    if (!over || active.id === over.id) return;
    const from = list.findIndex((rule) => rule.id === active.id);
    const to = list.findIndex((rule) => rule.id === over.id);
    reorder.mutate(arrayMove(list, from, to).map((rule) => rule.id));
  };

  const tooFewServers = servers.length < 2;

  return (
    <div className="pl-4 sm:pl-7">
      {rules.isSuccess && list.length === 0 && (
        <p className="border-t border-divider px-5 py-4 text-sm text-muted-foreground">
          {tooFewServers
            ? t`Add a second stream server to use combinations, e.g. a two-camera layout while both are live.`
            : t`None yet. Add one for, say, a two-camera layout while two servers are live.`}
        </p>
      )}

      {list.length > 0 && (
        <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
          <SortableContext
            items={list.map((rule) => rule.id)}
            strategy={verticalListSortingStrategy}
          >
            <ol>
              {list.map((rule, index) => (
                <RuleRow
                  key={rule.id}
                  rule={rule}
                  label={ruleLabel(rule, servers)}
                  shadowed={shadowedBy(list, index)}
                  servers={servers}
                  obs={obs}
                  inUse={rule.id === status?.active_scene_rule_id}
                  onEdit={() => setEditing({ rule })}
                />
              ))}
            </ol>
          </SortableContext>
        </DndContext>
      )}

      {canManage && (
        <Button
          variant="ghost"
          className="min-h-11 w-full justify-start rounded-none border-t border-divider px-5 text-muted-foreground"
          onClick={() => setEditing({})}
          disabled={tooFewServers || list.length >= MAX_RULES}
        >
          <Plus />
          <Trans>Add a server combination</Trans>
        </Button>
      )}
      <p className="border-t border-divider px-5 py-3 text-xs text-muted-foreground">{t`Use other scenes while certain servers are live together. If more than one matches, the highest one wins.`}</p>
      {canManage && editing && (
        <RuleDialog
          key={editing.rule?.id ?? "new"}
          rule={editing.rule}
          rules={list}
          servers={servers}
          obs={obs}
          onClose={() => setEditing(null)}
        />
      )}
    </div>
  );
}

function sceneName(obs: ObsScenes | undefined, uuid: string | null | undefined) {
  if (!uuid) return null;
  return obs?.scenes.find((scene) => scene.uuid === uuid)?.name ?? t`another scene`;
}

function useSaveRule(rule: SceneRule | undefined) {
  const { workspace, switcher, path } = useSwitcher();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: Draft & { enabled: boolean }) =>
      rule
        ? unwrap(
            api.PUT(
              "/workspaces/{workspace_id}/switchers/{switcher_id}/scene-rules/{scene_rule_id}",
              {
                params: { path: { ...path, scene_rule_id: rule.id } },
                body,
              },
            ),
          )
        : unwrap(
            api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/scene-rules", {
              params: { path },
              body,
            }),
          ),
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: queryKeys.sceneRules(workspace.id, switcher.id) }),
  });
}

function RuleRow({
  rule,
  label,
  shadowed,
  servers,
  obs,
  inUse,
  onEdit,
}: {
  rule: SceneRule;
  label: string;
  shadowed: SceneRule | undefined;
  servers: StreamServer[];
  obs: ObsScenes | undefined;
  inUse: boolean;
  onEdit: () => void;
}) {
  const { workspace, switcher, canManage, path } = useSwitcher();
  const queryClient = useQueryClient();
  const sortable = useSortable({ id: rule.id, disabled: !canManage });
  const save = useSaveRule(rule);
  const off = disabledServers(rule, servers);
  const remove = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE(
          "/workspaces/{workspace_id}/switchers/{switcher_id}/scene-rules/{scene_rule_id}",
          { params: { path: { ...path, scene_rule_id: rule.id } } },
        ),
      ),
    onSuccess: () => toast.success(t`Removed ${label}`),
    onError: (error) => toast.error(errorMessage(error)),
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: queryKeys.sceneRules(workspace.id, switcher.id) }),
  });

  const offNames = off.map((server) => server.name).join(", ");
  const warnings = [
    shadowed &&
      rule.enabled &&
      t`Never used: ${ruleLabel(shadowed, servers)} is above it and always matches first.`,
    off.length > 0 &&
      rule.enabled &&
      plural(off.length, {
        one: `Not used while ${offNames} is turned off.`,
        other: `Not used while ${offNames} are turned off.`,
      }),
  ].filter((warning): warning is string => Boolean(warning));

  return (
    <li
      ref={sortable.setNodeRef}
      style={{
        transform: sortable.transform
          ? `translate3d(${sortable.transform.x}px, ${sortable.transform.y}px, 0)`
          : undefined,
        transition: sortable.transition,
      }}
      className={cn(
        "flex flex-wrap items-center border-t border-divider",
        sortable.isDragging && "relative z-10 rounded-2xl border border-primary shadow-2xl",
      )}
    >
      {canManage && (
        <button
          type="button"
          ref={sortable.setActivatorNodeRef}
          {...sortable.attributes}
          {...sortable.listeners}
          aria-label={t`Move ${label}`}
          className="flex w-9 shrink-0 cursor-grab touch-none items-center justify-center text-subtle-foreground hover:text-foreground active:cursor-grabbing"
        >
          <GripVertical className="size-4" />
        </button>
      )}
      <div
        className={cn(
          "flex min-w-40 flex-1 flex-col gap-1.5 py-3 pr-3",
          !canManage && "pl-5",
          !rule.enabled && "opacity-60",
        )}
      >
        <span className="flex min-w-0 flex-wrap items-center gap-x-2.5 gap-y-1">
          <span className="min-w-0 text-sm font-semibold break-words">{t`While ${label} both have a feed`}</span>
        </span>
        <span className="text-xs text-muted-foreground">
          {inUse ? t`Server combination · In use now` : t`Server combination`}
        </span>
        {(["low", "offline"] as const).map(
          (role) =>
            rule.scenes[role] && (
              <span
                key={role}
                className="text-xs text-muted-foreground"
              >{t`${SCENE_ROLES[role].label}: ${sceneName(obs, rule.scenes[role]) ?? t`server’s scene`}`}</span>
            ),
        )}
        {warnings.map((warning) => (
          <span key={warning} className="flex gap-1.5 text-[13px] text-low">
            <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />
            {warning}
          </span>
        ))}
      </div>
      <div className="w-full px-5 pb-3 sm:w-auto sm:px-2 sm:py-3">
        <ScenePicker
          label={label}
          value={rule.scenes.live ?? null}
          obs={obs}
          disabled={!canManage || !obs || save.isPending}
          onChange={(live) =>
            save.mutate(
              {
                server_ids: rule.server_ids,
                scenes: { ...rule.scenes, live },
                enabled: rule.enabled,
              },
              { onError: (error) => toast.error(errorMessage(error)) },
            )
          }
        />
      </div>
      {canManage && (
        <span className="flex shrink-0 items-center gap-1 px-3 pb-3 sm:pb-0">
          <Switch
            checked={rule.enabled}
            onCheckedChange={(enabled) =>
              save.mutate(
                { server_ids: rule.server_ids, scenes: rule.scenes, enabled },
                { onError: (error) => toast.error(errorMessage(error)) },
              )
            }
            disabled={save.isPending}
            aria-label={t`Use ${label}`}
          />
          <Button variant="ghost" size="icon-sm" onClick={onEdit} aria-label={t`Edit ${label}`}>
            <Pencil />
          </Button>
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button variant="ghost" size="icon-sm" aria-label={t`Remove ${label}`}>
                <Trash2 />
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>
                  <Trans>Remove {label}</Trans>?
                </AlertDialogTitle>
                <AlertDialogDescription>
                  <Trans>
                    While these servers are live, NOALBS goes back to using the top server’s scenes.
                  </Trans>
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>
                  <Trans>Cancel</Trans>
                </AlertDialogCancel>
                <AlertDialogAction variant="destructive" onClick={() => remove.mutate()}>
                  <Trans>Remove</Trans>
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </span>
      )}
    </li>
  );
}

function RuleDialog({
  rule,
  rules,
  servers,
  obs,
  onClose,
}: {
  rule: SceneRule | undefined;
  rules: SceneRule[];
  servers: StreamServer[];
  obs: ObsScenes | undefined;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState<Draft>({
    server_ids: rule?.server_ids ?? [],
    scenes: rule?.scenes ?? { live: null, low: null, offline: null },
  });
  const save = useSaveRule(rule);
  const duplicate = findDuplicate(rules, draft.server_ids, rule?.id);
  const valid = draft.server_ids.length >= 2 && !duplicate;

  const toggleServer = (id: string) =>
    setDraft({
      ...draft,
      server_ids: draft.server_ids.includes(id)
        ? draft.server_ids.filter((other) => other !== id)
        : [...draft.server_ids, id],
    });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!valid) return;
    save.mutate(
      { ...draft, enabled: rule?.enabled ?? true },
      {
        onSuccess: () => {
          toast.success(t`Saved`);
          onClose();
        },
      },
    );
  };

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[90dvh] overflow-y-auto">
        <form onSubmit={submit} className="flex flex-col gap-5">
          <DialogHeader>
            <DialogTitle>
              {rule ? t`Edit server combination` : t`Add server combination`}
            </DialogTitle>
            <DialogDescription>
              <Trans>Used while all the servers you pick are live, even if others are too.</Trans>
            </DialogDescription>
          </DialogHeader>

          <fieldset className="flex flex-col gap-2">
            <legend className="mb-2 text-sm font-semibold">
              <Trans>Servers</Trans>
            </legend>
            <div className="flex flex-wrap gap-2">
              {servers.map((server) => {
                const picked = draft.server_ids.includes(server.id);
                return (
                  <button
                    key={server.id}
                    type="button"
                    aria-pressed={picked}
                    onClick={() => toggleServer(server.id)}
                    className={cn(
                      "flex h-9 items-center gap-1.5 rounded-full border px-3 text-sm",
                      picked
                        ? "border-primary bg-primary-soft font-medium text-primary-soft-foreground"
                        : "text-muted-foreground hover:text-foreground",
                    )}
                  >
                    {picked && <Check className="size-3.5" />}
                    {server.name}
                    {!server.enabled && (
                      <span className="text-xs text-subtle-foreground">
                        <Trans>(off)</Trans>
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
            <p className={cn("text-[13px] text-muted-foreground", duplicate && "text-low")}>
              {duplicate
                ? t`There is already a combination of these servers.`
                : t`Pick two or more.`}
            </p>
          </fieldset>

          <fieldset className="flex flex-col gap-3">
            <legend className="mb-1 text-sm font-semibold">
              <Trans>Scenes</Trans>
            </legend>
            {ROLES.map((role) => (
              <div key={role} className="flex flex-wrap items-center justify-between gap-2">
                <span className="flex items-center gap-2 text-sm font-medium">
                  <span className={cn("size-2 rounded-full", SCENE_ROLES[role].dot)} />
                  {SCENE_ROLES[role].label}
                </span>
                <ScenePicker
                  label={t`${SCENE_ROLES[role].label} for this combination`}
                  value={draft.scenes[role] ?? null}
                  obs={obs}
                  disabled={!obs}
                  noneLabel={t`Server’s scene`}
                  onChange={(uuid) =>
                    setDraft({ ...draft, scenes: { ...draft.scenes, [role]: uuid } })
                  }
                />
              </div>
            ))}
            <p className="text-[13px] text-muted-foreground">
              <Trans>
                “Server’s scene” keeps what the top server would use: its override, or your scene
                above.
              </Trans>
            </p>
          </fieldset>

          {save.error && <p className="text-sm text-destructive">{errorMessage(save.error)}</p>}

          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              <Trans>Cancel</Trans>
            </Button>
            <Button type="submit" disabled={!valid || save.isPending}>
              {save.isPending ? t`Saving…` : t`Save combination`}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
