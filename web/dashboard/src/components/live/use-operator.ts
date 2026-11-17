import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { eventsQuery, obsScenesQuery, queryKeys } from "@/api/queries";
import type { SwitcherEvent, SwitcherStatus } from "@/api/types";
import { useMediaQuery } from "@/hooks/use-media-query";
import { errorMessage } from "@/lib/errors";
import { sceneKey, useHiddenScenes } from "@/lib/hidden-scenes";
import {
  operatorHotkey,
  operatorKey,
  readOperator,
  type Scene,
  swapAfterTake,
  switchedBy,
} from "./operator-state";

type Path = { workspace_id: string; switcher_id: string };
const immersiveKey = "noalbs-live-immersive:v2";
const modeKey = "noalbs-live-operator-mode:v1";
const readImmersive = () => {
  try {
    return localStorage.getItem(immersiveKey) === "true";
  } catch {
    return false;
  }
};

export function useOperator(
  path: Path,
  status: SwitcherStatus | null | undefined,
  disabled: boolean,
  events: SwitcherEvent[],
  lastKnownScene?: SwitcherStatus["current_scene"],
) {
  const desktop = useMediaQuery("(min-width: 768px)");
  const [mode, setMode] = useState(() => {
    try {
      return localStorage.getItem(modeKey) === "true";
    } catch {
      return false;
    }
  });
  const enabled = desktop && mode;
  useEffect(() => {
    try {
      localStorage.setItem(modeKey, String(mode));
    } catch {}
  }, [mode]);
  const [saved, setSaved] = useState(() => ({
    id: path.switcher_id,
    value: readOperator(path.switcher_id),
  }));
  const state = saved.id === path.switcher_id ? saved.value : readOperator(path.switcher_id);
  const update: typeof setState = (next) =>
    setSaved((previous) => ({
      id: path.switcher_id,
      value:
        typeof next === "function"
          ? next(previous.id === path.switcher_id ? previous.value : readOperator(path.switcher_id))
          : next,
    }));
  function setState(next: typeof state | ((previous: typeof state) => typeof state)) {
    update(next);
  }
  const [help, setHelp] = useState(false);
  const [immersive, setImmersive] = useState(readImmersive);
  const root = useRef<HTMLDivElement>(null);
  const knownProgram = useRef<{ switcher: string; scene: Scene | null }>({
    switcher: path.switcher_id,
    scene: null,
  });
  if (knownProgram.current.switcher !== path.switcher_id)
    knownProgram.current = { switcher: path.switcher_id, scene: null };
  const reportedScene = status?.current_scene ?? lastKnownScene;
  if (reportedScene?.uuid)
    knownProgram.current.scene = { uuid: reportedScene.uuid, name: reportedScene.name };
  const program = status?.current_scene?.uuid
    ? { uuid: status.current_scene.uuid, name: status.current_scene.name }
    : knownProgram.current.scene;
  const [hidden] = useHiddenScenes(path.switcher_id);
  const scenes = useQuery({
    ...obsScenesQuery(path.workspace_id, path.switcher_id),
    enabled: Boolean(status?.obs_connected),
  });
  const validScenes =
    scenes.data?.scenes.filter((scene): scene is Scene => Boolean(scene.uuid)) ?? [];
  const visible = validScenes.filter(
    (scene) => !hidden.includes(sceneKey(scene)) || scene.uuid === program?.uuid,
  );
  const preview = validScenes.find((scene) => scene.uuid === state.preview?.uuid) ?? null;
  const backlog = useInfiniteQuery(eventsQuery(path.workspace_id, path.switcher_id));
  const by = switchedBy([...events, ...(backlog.data?.pages.flat() ?? [])], program?.name);
  const client = useQueryClient();
  const switching = useRef(false);
  const mutation = useMutation({
    scope: { id: `scene-switch-${path.switcher_id}` },
    mutationFn: ({ target }: { target: Scene; previous: Scene | null; swap: boolean }) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scene", {
          params: { path },
          body: { scene_uuid: target.uuid },
        }),
      ),
    onSuccess: (_, variables) => {
      if (variables.swap) setState((previous) => swapAfterTake(previous, variables.previous));
    },
    onError: (error) => toast.error(errorMessage(error)),
    onSettled: () => {
      switching.current = false;
      void client.invalidateQueries({
        queryKey: queryKeys.events(path.workspace_id, path.switcher_id),
      });
      void client.invalidateQueries({
        queryKey: queryKeys.obsScenes(path.workspace_id, path.switcher_id),
      });
    },
  });
  const cue = (scene: Scene) => {
    if (!disabled) setState((previous) => ({ ...previous, preview: scene }));
  };
  const cutScene = (target: Scene, swap = false) => {
    if (disabled || switching.current || target.uuid === program?.uuid) return;
    switching.current = true;
    mutation.mutate({ target, previous: program, swap });
  };
  const take = () => {
    if (enabled && state.studio && preview) cutScene(preview, true);
  };
  const exitImmersive = () => {
    setImmersive(false);
  };
  const toggleImmersive = () => {
    setImmersive((value) => !value);
  };
  useEffect(() => {
    try {
      localStorage.setItem(operatorKey(path.switcher_id), JSON.stringify(state));
    } catch {}
  }, [path.switcher_id, state]);
  useEffect(() => {
    document.body.classList.toggle("live-immersive", immersive);
    try {
      localStorage.setItem(immersiveKey, String(immersive));
    } catch {}
    window.dispatchEvent(new Event("resize"));
    return () => document.body.classList.remove("live-immersive");
  }, [immersive]);
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      const target = event.target instanceof Element ? event.target : null;
      const dialog = Boolean(document.querySelector('[role="dialog"], [role="alertdialog"]'));
      const editing = Boolean(root.current?.querySelector('[data-editing="true"]'));
      if (event.key === "Escape" && immersive && !dialog && !editing && !event.defaultPrevented) {
        exitImmersive();
        return;
      }
      if (!enabled && !(immersive && event.key.toLowerCase() === "f")) return;
      const action = operatorHotkey(event, {
        desktop,
        editing,
        blocked:
          dialog ||
          Boolean(
            target?.closest(
              'input, textarea, select, [contenteditable]:not([contenteditable="false"]), [role="textbox"], [role="combobox"]',
            ),
          ),
        hold: Boolean(target?.closest("[data-live-hold]")),
      });
      if (!action) return;
      event.preventDefault();
      event.stopPropagation();
      if (event.repeat) return;
      if (action.action === "help") setHelp(true);
      else if (action.action === "search")
        root.current?.querySelector<HTMLInputElement>("[data-scene-search]")?.focus();
      else if (action.action === "immersive") toggleImmersive();
      else if (action.action === "take" || action.action === "cut") take();
      else if (action.action === "scene") {
        const scene = visible[action.index];
        if (scene) {
          if (state.studio && !action.direct) cue(scene);
          else cutScene(scene);
        }
      }
    };
    window.addEventListener("keydown", keydown, true);
    return () => window.removeEventListener("keydown", keydown, true);
  });
  return {
    root,
    enabled,
    setMode,
    desktop,
    state,
    setState,
    studio: enabled && state.studio,
    preview,
    program,
    by,
    cue,
    cutScene,
    take,
    disabled,
    pending: mutation.isPending,
    help: enabled && help,
    setHelp,
    immersive,
    toggleImmersive,
  };
}
