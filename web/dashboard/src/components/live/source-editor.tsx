import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus, X } from "lucide-react";
import { type FormEvent, useId, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { obsScenesQuery, queryKeys } from "@/api/queries";
import type { ObsSource } from "@/api/types";
import { Field, MoreSettings, NumberField, Toggle } from "@/components/live/source-fields";
import { SOURCE_ICONS } from "@/components/live/source-icons";
import { SourcePosition } from "@/components/live/source-position";
import { SecretUrlInput } from "@/components/secret-url-input";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { errorMessage } from "@/lib/errors";
import { copyName, type EditableKind, inputKind } from "@/lib/obs-sources";
import {
  type BrowserSettings,
  type MediaSettings,
  newSettings,
  playsLocalFile,
  type SourceSettings,
  settingsToSave,
  type TextSettings,
  type VlcSettings,
  whatItShows,
} from "@/lib/source-editor";
import { cn } from "@/lib/utils";

type Path = { workspace_id: string; switcher_id: string };

/**
 * What the editor is open for: adding a source to a scene, editing one by
 * name, or duplicating one (`taken`: the names in use, for the copy's).
 */
export type EditorTarget =
  | { mode: "create"; scene: string }
  | { mode: "edit"; name: string }
  | { mode: "duplicate"; source: ObsSource; scene: string; taken: string[] }
  | { mode: "position"; source: ObsSource };

const kindLabels = (): Record<EditableKind, string> => ({
  media: t`Media source`,
  vlc: t`VLC video source`,
  browser: t`Browser source`,
  text: t`Text`,
});

/**
 * Adds a media, VLC, browser or text source to a scene, or edits one: what it plays
 * (stream URLs only, shown hidden: they can hold keys) and OBS's settings for
 * it that matter on stream. A source playing a file on the OBS computer keeps
 * it unless a URL replaces it.
 */
export function SourceEditorDialog({
  path,
  target,
  onClose,
}: {
  path: Path;
  target: EditorTarget | null;
  onClose: () => void;
}) {
  return (
    <Dialog open={target !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="flex max-h-[88dvh] flex-col gap-4 sm:max-w-lg">
        {target?.mode === "create" && (
          <CreateSource key={target.scene} path={path} scene={target.scene} onDone={onClose} />
        )}
        {target?.mode === "edit" && (
          <EditSource key={target.name} path={path} name={target.name} onDone={onClose} />
        )}
        {target?.mode === "position" && (
          <SourcePosition
            key={`${target.source.parent}:${target.source.item_id}`}
            path={path}
            source={target.source}
          />
        )}
        {target?.mode === "duplicate" && (
          <DuplicateSource
            key={target.source.name}
            path={path}
            source={target.source}
            scene={target.scene}
            taken={target.taken}
            onDone={onClose}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

function useSourceSaved(path: Path, onDone: () => void) {
  const queryClient = useQueryClient();
  return () => {
    void queryClient.invalidateQueries({
      queryKey: queryKeys.obsSources(path.workspace_id, path.switcher_id),
    });
    // A browser source routing its audio through OBS shows on the mixer.
    void queryClient.invalidateQueries({
      queryKey: queryKeys.obsAudio(path.workspace_id, path.switcher_id),
    });
    onDone();
  };
}

/** Adding to a scene: a new source, or one OBS already has (or a scene). */
function CreateSource({ path, scene, onDone }: { path: Path; scene: string; onDone: () => void }) {
  const [from, setFrom] = useState<"new" | "existing">("new");
  return (
    <>
      <DialogHeader>
        <DialogTitle>
          <Trans>Add a source</Trans>
        </DialogTitle>
        <DialogDescription>
          <Trans>It’s added on top of the scene in OBS, so it shows over the others.</Trans>
        </DialogDescription>
      </DialogHeader>
      <div className="grid grid-cols-2 rounded-lg border p-0.5">
        {(
          [
            ["new", t`New source`],
            ["existing", t`Existing source`],
          ] as const
        ).map(([value, label]) => (
          <button
            key={value}
            type="button"
            aria-pressed={from === value}
            onClick={() => setFrom(value)}
            className={cn(
              "h-8 rounded-md text-sm font-medium outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
              from === value
                ? "bg-primary-soft font-semibold text-primary-soft-foreground"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {label}
          </button>
        ))}
      </div>
      {from === "new" ? (
        <NewSource path={path} scene={scene} onDone={onDone} />
      ) : (
        <AddExisting path={path} scene={scene} onDone={onDone} />
      )}
    </>
  );
}

/** The sources OBS has that can be added to a scene (shared by adding and duplicating). */
function useExistingSources(path: Path) {
  return useQuery({
    queryKey: [...queryKeys.obsSources(path.workspace_id, path.switcher_id), "existing"],
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/existing-sources", {
          params: { path },
        }),
      ),
    retry: false,
  });
}

/** The scene to add to: the scenes in OBS (`scene` shows before they load). */
function SceneField({
  path,
  scene,
  value,
  onChange,
}: {
  path: Path;
  scene: string;
  value: string;
  onChange: (scene: string) => void;
}) {
  const id = useId();
  const scenes = useQuery(obsScenesQuery(path.workspace_id, path.switcher_id));
  return (
    <Field id={id} label={t`Scene`}>
      <Select value={value} onValueChange={onChange}>
        <SelectTrigger id={id} className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {!scenes.data?.scenes.some((item) => item.name === scene) && (
            <SelectItem value={scene}>{scene}</SelectItem>
          )}
          {scenes.data?.scenes.map((item) => (
            <SelectItem key={item.uuid ?? item.name} value={item.name}>
              {item.name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Field>
  );
}

/**
 * A source OBS already has, or a scene, added to another scene: the same
 * source, so a change to it shows in every scene it's in.
 */
function AddExisting({ path, scene, onDone }: { path: Path; scene: string; onDone: () => void }) {
  const ids = useId();
  const saved = useSourceSaved(path, onDone);
  const existing = useExistingSources(path);
  const [sceneName, setSceneName] = useState(scene);
  const [search, setSearch] = useState("");
  const [picked, setPicked] = useState<string | null>(null);
  const [enabled, setEnabled] = useState(true);

  const add = useMutation({
    mutationFn: (source: string) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scene-items", {
          params: { path },
          body: { scene: sceneName, source, enabled },
        }),
      ),
    onSuccess: (added) => {
      toast.success(t`Added ${added.name} to ${sceneName}`);
      saved();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const needle = search.trim().toLowerCase();
  // A scene can't go into itself.
  const shown = (existing.data?.sources ?? []).filter(
    (source) =>
      !(source.is_scene && source.name === sceneName) && source.name.toLowerCase().includes(needle),
  );

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (picked) add.mutate(picked);
  };

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-col gap-4">
      <div className="-mx-1 flex min-h-0 flex-col gap-4 overflow-y-auto px-1">
        <p className="text-xs text-muted-foreground">
          <Trans>
            It’s the same source, not a copy: changing it changes it in every scene it’s in. To
            change one on its own, duplicate it instead.
          </Trans>
        </p>
        <SceneField path={path} scene={scene} value={sceneName} onChange={setSceneName} />
        <Field id={`${ids}-search`} label={t`Source`}>
          <Input
            id={`${ids}-search`}
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={t`Search`}
            autoComplete="off"
          />
        </Field>
        {existing.isPending ? (
          <Skeleton className="h-40" />
        ) : existing.isError ? (
          <p className="text-sm text-destructive">{errorMessage(existing.error)}</p>
        ) : shown.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            <Trans>Nothing matches.</Trans>
          </p>
        ) : (
          <div className="flex max-h-64 flex-col divide-y divide-divider overflow-y-auto rounded-lg border">
            {shown.map((source) => {
              const Icon =
                SOURCE_ICONS[source.is_scene ? "scene" : inputKind(source.input_kind ?? "")];
              const key = `${source.is_scene}:${source.name}`;
              return (
                <button
                  key={key}
                  type="button"
                  aria-pressed={picked === source.name}
                  onClick={() => setPicked(source.name)}
                  className={cn(
                    "flex min-h-10 items-center gap-2 px-3 py-1.5 text-left text-sm",
                    picked === source.name
                      ? "bg-primary-soft font-semibold text-primary-soft-foreground"
                      : "hover:bg-surface-2",
                  )}
                >
                  <Icon className="size-4 shrink-0" aria-hidden="true" />
                  <span className="min-w-0 flex-1 truncate">{source.name}</span>
                  {source.is_scene && (
                    <span className="text-xs text-muted-foreground">
                      <Trans>Scene</Trans>
                    </span>
                  )}
                </button>
              );
            })}
          </div>
        )}
        <Toggle
          id={`${ids}-enabled`}
          label={t`Show it right away`}
          checked={enabled}
          onChange={setEnabled}
        />
      </div>
      <DialogFooter>
        <Button type="submit" disabled={!picked || add.isPending}>
          <Trans>Add source</Trans>
        </Button>
      </DialogFooter>
    </form>
  );
}

/** A copy of a source with its own settings, filters and placement, like OBS's Duplicate. */
function DuplicateSource({
  path,
  source,
  scene,
  taken,
  onDone,
}: {
  path: Path;
  source: ObsSource;
  scene: string;
  taken: string[];
  onDone: () => void;
}) {
  const ids = useId();
  const saved = useSourceSaved(path, onDone);
  // Suggested until it's typed over, from every name OBS has once they load.
  const existing = useExistingSources(path);
  const [typed, setTyped] = useState<string | null>(null);
  const name =
    typed ??
    copyName(source.name, [...taken, ...(existing.data?.sources.map((item) => item.name) ?? [])]);
  const setName = setTyped;
  const [sceneName, setSceneName] = useState(scene);
  const [enabled, setEnabled] = useState(source.enabled);

  const duplicate = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/source/duplicate", {
          params: { path },
          body: {
            parent: source.parent,
            parent_is_group: source.parent_is_group,
            item_id: source.item_id,
            name,
            scene: sceneName,
            enabled,
          },
        }),
      ),
    onSuccess: (copy) => {
      toast.success(t`Copied ${source.name} as ${copy.name}`);
      saved();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    duplicate.mutate();
  };

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-col gap-4">
      <DialogHeader>
        <DialogTitle>
          <Trans>Duplicate {source.name}</Trans>
        </DialogTitle>
        <DialogDescription>
          <Trans>
            A new source with a copy of its settings, filters and placement. Changing one doesn’t
            change the other.
          </Trans>
        </DialogDescription>
      </DialogHeader>
      <div className="-mx-1 flex min-h-0 flex-col gap-4 overflow-y-auto px-1">
        <Field id={`${ids}-name`} label={t`Name of the copy`}>
          <Input
            id={`${ids}-name`}
            value={name}
            onChange={(event) => setName(event.target.value)}
            autoComplete="off"
            required
          />
        </Field>
        <SceneField path={path} scene={scene} value={sceneName} onChange={setSceneName} />
        <Toggle
          id={`${ids}-enabled`}
          label={t`Show it right away`}
          checked={enabled}
          onChange={setEnabled}
        />
      </div>
      <DialogFooter>
        <Button type="submit" disabled={duplicate.isPending}>
          <Trans>Duplicate</Trans>
        </Button>
      </DialogFooter>
    </form>
  );
}

/** A new media, VLC, browser or text source. */
function NewSource({ path, scene, onDone }: { path: Path; scene: string; onDone: () => void }) {
  const ids = useId();
  const saved = useSourceSaved(path, onDone);
  const kinds = useQuery({
    queryKey: [...queryKeys.obsScenes(path.workspace_id, path.switcher_id), "source-kinds"],
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/source-kinds", {
          params: { path },
        }),
      ),
    retry: false,
  });
  const [sceneName, setSceneName] = useState(scene);
  const [name, setName] = useState("");
  const [enabled, setEnabled] = useState(true);
  const [settings, setSettings] = useState<SourceSettings>(newSettings("media"));

  const create = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/inputs", {
          params: { path },
          body: { scene: sceneName, name, enabled, settings: settingsToSave(settings, true) },
        }),
      ),
    onSuccess: (created) => {
      toast.success(t`Added ${created.name} to ${sceneName}`);
      saved();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    create.mutate();
  };

  const available = kinds.data?.kinds ?? [];
  const labels = kindLabels();

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-col gap-4">
      <div className="-mx-1 flex min-h-0 flex-col gap-4 overflow-y-auto px-1">
        <fieldset className="flex flex-col gap-1.5">
          <legend className="mb-1.5 text-sm font-medium">
            <Trans>Kind</Trans>
          </legend>
          {kinds.isPending ? (
            <Skeleton className="h-9" />
          ) : kinds.isError ? (
            <p className="text-sm text-destructive">{errorMessage(kinds.error)}</p>
          ) : (
            <div className="grid grid-cols-2 gap-1.5 sm:grid-cols-4">
              {(["media", "vlc", "browser", "text"] as const).map((kind) => {
                const has = available.includes(kind);
                return (
                  <button
                    key={kind}
                    type="button"
                    aria-pressed={settings.kind === kind}
                    disabled={!has}
                    title={
                      has
                        ? undefined
                        : kind === "vlc"
                          ? t`OBS doesn’t have the VLC plugin`
                          : t`This OBS doesn’t have it`
                    }
                    onClick={() => setSettings(newSettings(kind))}
                    className={cn(
                      "min-h-9 rounded-lg border px-2 py-1.5 text-xs font-semibold disabled:cursor-not-allowed disabled:opacity-50",
                      settings.kind === kind
                        ? "border-2 border-primary bg-primary-soft text-primary-soft-foreground"
                        : "hover:bg-surface-2",
                    )}
                  >
                    {labels[kind]}
                  </button>
                );
              })}
            </div>
          )}
          {kinds.data && !available.includes("vlc") && (
            <p className="text-xs text-muted-foreground">
              <Trans>VLC video sources need OBS’s VLC plugin and VLC installed.</Trans>
            </p>
          )}
        </fieldset>

        <Field id={`${ids}-name`} label={t`Name`}>
          <Input
            id={`${ids}-name`}
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder={
              {
                media: t`Backup SRT`,
                vlc: t`Backup SRT`,
                browser: t`Chat overlay`,
                text: t`Status`,
              }[settings.kind]
            }
            autoComplete="off"
            required
          />
        </Field>

        <SceneField path={path} scene={scene} value={sceneName} onChange={setSceneName} />

        <SettingsFields settings={settings} onChange={setSettings} replacing creating />

        <Toggle
          id={`${ids}-enabled`}
          label={t`Show it right away`}
          checked={enabled}
          onChange={setEnabled}
        />
      </div>

      <DialogFooter>
        <Button type="submit" disabled={create.isPending || !kinds.data}>
          <Trans>Add source</Trans>
        </Button>
      </DialogFooter>
    </form>
  );
}

function EditSource({ path, name, onDone }: { path: Path; name: string; onDone: () => void }) {
  const input = useQuery({
    queryKey: [...queryKeys.obsSources(path.workspace_id, path.switcher_id), "input", name],
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/input", {
          params: { path, query: { name } },
        }),
      ),
    retry: false,
    // It can hold a stream key: don't keep it around.
    gcTime: 0,
    staleTime: Infinity,
  });

  if (input.isPending) {
    return (
      <>
        <DialogHeader>
          <DialogTitle>{name}</DialogTitle>
          <DialogDescription>
            <Trans>Loading its settings from OBS…</Trans>
          </DialogDescription>
        </DialogHeader>
        <Skeleton className="h-48" />
      </>
    );
  }
  if (input.isError) {
    return (
      <DialogHeader>
        <DialogTitle>{name}</DialogTitle>
        <DialogDescription>{errorMessage(input.error)}</DialogDescription>
      </DialogHeader>
    );
  }
  return <EditForm path={path} name={name} loaded={input.data.settings} onDone={onDone} />;
}

function EditForm({
  path,
  name,
  loaded,
  onDone,
}: {
  path: Path;
  name: string;
  loaded: SourceSettings;
  onDone: () => void;
}) {
  const ids = useId();
  const saved = useSourceSaved(path, onDone);
  const [newName, setNewName] = useState(name);
  const [settings, setSettings] = useState(loaded);
  const local = playsLocalFile(loaded);
  // Whether what it plays changes: a source's file is kept until a URL replaces it.
  const [replacing, setReplacing] = useState(!local);
  const replaced =
    replacing && JSON.stringify(whatItShows(settings)) !== JSON.stringify(whatItShows(loaded));

  const save = useMutation({
    mutationFn: () =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/input", {
          params: { path },
          body: { name, new_name: newName, settings: settingsToSave(settings, replaced) },
        }),
      ),
    onSuccess: (result) => {
      toast.success(t`Saved ${result.name}`);
      saved();
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    save.mutate();
  };

  const startReplacing = () => {
    setReplacing(true);
    setSettings((current) => {
      switch (current.kind) {
        case "vlc":
          return { ...current, playlist: [""] };
        case "text":
          return { ...current, text: "" };
        default:
          return { ...current, url: "" };
      }
    });
  };

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-col gap-4">
      <DialogHeader>
        <DialogTitle>
          <Trans>Edit {name}</Trans>
        </DialogTitle>
        <DialogDescription>
          {kindLabels()[loaded.kind]} ·{" "}
          {loaded.kind === "text" ? (
            <Trans>Saving changes it in OBS right away.</Trans>
          ) : (
            <Trans>Saving changes it in OBS right away, and it reloads what it plays.</Trans>
          )}
        </DialogDescription>
      </DialogHeader>

      <div className="-mx-1 flex min-h-0 flex-col gap-4 overflow-y-auto px-1">
        <Field id={`${ids}-name`} label={t`Name`}>
          <Input
            id={`${ids}-name`}
            value={newName}
            onChange={(event) => setNewName(event.target.value)}
            autoComplete="off"
            required
          />
        </Field>

        {!replacing && (
          <div className="flex flex-col items-start gap-2 rounded-lg border border-dashed p-3 text-sm text-muted-foreground">
            {loaded.kind === "vlc" ? (
              <Trans>
                It plays files from the OBS computer. They can only be picked in OBS, and they’re
                kept unless you replace them with URLs.
              </Trans>
            ) : loaded.kind === "text" ? (
              <Trans>
                It shows the text of a file on the OBS computer. Files can only be picked in OBS,
                and it keeps reading it unless you type text instead.
              </Trans>
            ) : (
              <Trans>
                It plays a file from the OBS computer. Files can only be picked in OBS, and it’s
                kept unless you replace it with a URL.
              </Trans>
            )}
            <Button type="button" size="sm" variant="outline" onClick={startReplacing}>
              {loaded.kind === "vlc"
                ? t`Replace with URLs`
                : loaded.kind === "text"
                  ? t`Type text instead`
                  : t`Replace with a URL`}
            </Button>
          </div>
        )}

        <SettingsFields settings={settings} onChange={setSettings} replacing={replacing} />
      </div>

      <DialogFooter>
        <Button type="submit" disabled={save.isPending}>
          <Trans>Save</Trans>
        </Button>
      </DialogFooter>
    </form>
  );
}

/** The fields of one kind of source. What it plays shows only while `replacing`. */
function SettingsFields({
  settings,
  onChange,
  replacing,
  creating = false,
}: {
  settings: SourceSettings;
  onChange: (settings: SourceSettings) => void;
  replacing: boolean;
  creating?: boolean;
}) {
  switch (settings.kind) {
    case "media":
      return (
        <MediaFields
          settings={settings}
          onChange={onChange}
          replacing={replacing}
          creating={creating}
        />
      );
    case "vlc":
      return <VlcFields settings={settings} onChange={onChange} replacing={replacing} />;
    case "browser":
      return (
        <BrowserFields
          settings={settings}
          onChange={onChange}
          replacing={replacing}
          creating={creating}
        />
      );
    case "text":
      return <TextFields settings={settings} onChange={onChange} replacing={replacing} />;
  }
}

/**
 * Text (GDI+) on Windows and Text (FreeType 2) elsewhere have different settings;
 * these are what both have. Text isn't a secret: it's shown as is.
 */
function TextFields({
  settings,
  onChange,
  replacing,
}: {
  settings: TextSettings;
  onChange: (settings: TextSettings) => void;
  replacing: boolean;
}) {
  const ids = useId();
  const set = <K extends keyof TextSettings>(key: K, value: TextSettings[K]) =>
    onChange({ ...settings, [key]: value });
  return (
    <>
      {replacing && (
        <Field id={`${ids}-text`} label={t`Text`}>
          <textarea
            id={`${ids}-text`}
            value={settings.text ?? ""}
            onChange={(event) => set("text", event.target.value)}
            rows={3}
            className="w-full rounded-md border border-input bg-transparent px-3 py-2 text-base shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 md:text-sm dark:bg-input/30"
          />
        </Field>
      )}
      <div className="grid grid-cols-[1fr_auto] items-end gap-3">
        <NumberField
          id={`${ids}-size`}
          label={t`Font size`}
          value={settings.font_size}
          min={1}
          max={1024}
          onChange={(value) => set("font_size", value)}
        />
        <Field id={`${ids}-color`} label={t`Color`}>
          <input
            id={`${ids}-color`}
            type="color"
            value={settings.color}
            onChange={(event) => set("color", event.target.value)}
            className="h-9 w-16 cursor-pointer rounded-md border border-input bg-transparent p-1"
          />
        </Field>
      </div>
      <Toggle
        id={`${ids}-bold`}
        label={t`Bold`}
        checked={settings.bold}
        onChange={(value) => set("bold", value)}
      />
      <Toggle
        id={`${ids}-italic`}
        label={t`Italic`}
        checked={settings.italic}
        onChange={(value) => set("italic", value)}
      />
      <Toggle
        id={`${ids}-outline`}
        label={t`Outline`}
        checked={settings.outline}
        onChange={(value) => set("outline", value)}
      />
      <MoreSettings>
        <Field
          id={`${ids}-face`}
          label={t`Font`}
          hint={t`As the OBS computer names it, e.g. Arial. Empty keeps the font it has.`}
        >
          <Input
            id={`${ids}-face`}
            value={settings.font_face}
            onChange={(event) => set("font_face", event.target.value)}
            autoComplete="off"
            spellCheck={false}
          />
        </Field>
      </MoreSettings>
    </>
  );
}

function MediaFields({
  settings,
  onChange,
  replacing,
  creating,
}: {
  settings: MediaSettings;
  onChange: (settings: MediaSettings) => void;
  replacing: boolean;
  creating: boolean;
}) {
  const ids = useId();
  const set = <K extends keyof MediaSettings>(key: K, value: MediaSettings[K]) =>
    onChange({ ...settings, [key]: value });
  return (
    <>
      {replacing && (
        <Field
          id={`${ids}-url`}
          label={t`Stream URL`}
          hint={t`An srt://, rtmp://, rist://, udp:// or https:// address.`}
        >
          <SecretUrlInput
            id={`${ids}-url`}
            label={t`stream URL`}
            value={settings.url ?? ""}
            onChange={(url) => set("url", url)}
            placeholder="srt://127.0.0.1:4000?streamid=play/live/stream"
            startRevealed={creating}
          />
        </Field>
      )}
      <MoreSettings>
        <Field
          id={`${ids}-format`}
          label={t`Input format`}
          hint={t`Empty to detect it, or e.g. mpegts.`}
        >
          <Input
            id={`${ids}-format`}
            value={settings.input_format}
            onChange={(event) => set("input_format", event.target.value)}
            autoComplete="off"
            spellCheck={false}
          />
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <NumberField
            id={`${ids}-buffering`}
            label={t`Network buffering (MB)`}
            value={settings.buffering_mb}
            min={0}
            max={16}
            onChange={(value) => set("buffering_mb", value)}
          />
          <NumberField
            id={`${ids}-reconnect`}
            label={t`Reconnect delay (s)`}
            value={settings.reconnect_delay_sec}
            min={1}
            max={60}
            onChange={(value) => set("reconnect_delay_sec", value)}
          />
        </div>
        <Toggle
          id={`${ids}-restart`}
          label={t`Restart playback when the source becomes active`}
          checked={settings.restart_on_activate}
          onChange={(value) => set("restart_on_activate", value)}
        />
        <Toggle
          id={`${ids}-close`}
          label={t`Close the stream while it isn’t shown`}
          checked={settings.close_when_inactive}
          onChange={(value) => set("close_when_inactive", value)}
        />
        <Toggle
          id={`${ids}-clear`}
          label={t`Show nothing when playback ends`}
          checked={settings.clear_on_media_end}
          onChange={(value) => set("clear_on_media_end", value)}
        />
        <Toggle
          id={`${ids}-hw`}
          label={t`Use hardware decoding when available`}
          checked={settings.hw_decode}
          onChange={(value) => set("hw_decode", value)}
        />
      </MoreSettings>
    </>
  );
}

function VlcFields({
  settings,
  onChange,
  replacing,
}: {
  settings: VlcSettings;
  onChange: (settings: VlcSettings) => void;
  replacing: boolean;
}) {
  const ids = useId();
  const set = <K extends keyof VlcSettings>(key: K, value: VlcSettings[K]) =>
    onChange({ ...settings, [key]: value });
  const playlist = settings.playlist ?? [];
  const setUrl = (index: number, url: string) =>
    set(
      "playlist",
      playlist.map((item, at) => (at === index ? url : item)),
    );

  return (
    <>
      {replacing && (
        <fieldset className="flex flex-col gap-1.5">
          <legend className="mb-1.5 text-sm font-medium">
            <Trans>Playlist</Trans>
          </legend>
          {playlist.map((url, index) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: rows are edited in place
            <div key={index} className="flex gap-2">
              <div className="min-w-0 flex-1">
                <SecretUrlInput
                  id={`${ids}-url-${index}`}
                  label={t`stream URL`}
                  value={url}
                  onChange={(value) => setUrl(index, value)}
                  placeholder="srt://127.0.0.1:4000"
                  startRevealed={url === ""}
                />
              </div>
              {playlist.length > 1 && (
                <Button
                  type="button"
                  size="icon"
                  variant="ghost"
                  aria-label={t`Remove this URL`}
                  onClick={() =>
                    set(
                      "playlist",
                      playlist.filter((_, at) => at !== index),
                    )
                  }
                >
                  <X />
                </Button>
              )}
            </div>
          ))}
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="self-start"
            onClick={() => set("playlist", [...playlist, ""])}
          >
            <Plus />
            <Trans>Add URL</Trans>
          </Button>
        </fieldset>
      )}
      <Field id={`${ids}-playback`} label={t`While it isn’t shown`}>
        <Select
          value={settings.playback}
          onValueChange={(value) => set("playback", value as VlcSettings["playback"])}
        >
          <SelectTrigger id={`${ids}-playback`} className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="stop_restart">{t`Stop, and restart when shown`}</SelectItem>
            <SelectItem value="pause_unpause">{t`Pause, and resume when shown`}</SelectItem>
            <SelectItem value="always_play">{t`Keep playing`}</SelectItem>
          </SelectContent>
        </Select>
      </Field>
      <MoreSettings>
        <NumberField
          id={`${ids}-caching`}
          label={t`Network caching (ms)`}
          value={settings.network_caching_ms}
          min={100}
          max={60000}
          onChange={(value) => set("network_caching_ms", value)}
        />
        <Toggle
          id={`${ids}-loop`}
          label={t`Loop the playlist`}
          checked={settings.loop_playlist}
          onChange={(value) => set("loop_playlist", value)}
        />
        <Toggle
          id={`${ids}-shuffle`}
          label={t`Shuffle`}
          checked={settings.shuffle}
          onChange={(value) => set("shuffle", value)}
        />
      </MoreSettings>
    </>
  );
}

function BrowserFields({
  settings,
  onChange,
  replacing,
  creating,
}: {
  settings: BrowserSettings;
  onChange: (settings: BrowserSettings) => void;
  replacing: boolean;
  creating: boolean;
}) {
  const ids = useId();
  const set = <K extends keyof BrowserSettings>(key: K, value: BrowserSettings[K]) =>
    onChange({ ...settings, [key]: value });
  return (
    <>
      {replacing && (
        <Field
          id={`${ids}-url`}
          label={t`URL`}
          hint={t`Overlay links often hold a key: it’s shown hidden.`}
        >
          <SecretUrlInput
            id={`${ids}-url`}
            label={t`page URL`}
            value={settings.url ?? ""}
            onChange={(url) => set("url", url)}
            placeholder="https://"
            startRevealed={creating}
          />
        </Field>
      )}
      <div className="grid grid-cols-2 gap-3">
        <NumberField
          id={`${ids}-width`}
          label={t`Width`}
          value={settings.width}
          min={1}
          max={8192}
          onChange={(value) => set("width", value)}
        />
        <NumberField
          id={`${ids}-height`}
          label={t`Height`}
          value={settings.height}
          min={1}
          max={8192}
          onChange={(value) => set("height", value)}
        />
      </div>
      <Toggle
        id={`${ids}-shutdown`}
        label={t`Shut it down while it isn’t shown`}
        checked={settings.shutdown}
        onChange={(value) => set("shutdown", value)}
      />
      <Toggle
        id={`${ids}-restart`}
        label={t`Reload it when its scene goes on air`}
        checked={settings.restart_when_active}
        onChange={(value) => set("restart_when_active", value)}
      />
      <MoreSettings>
        <Toggle
          id={`${ids}-audio`}
          label={t`Control its audio from OBS’s Audio Mixer`}
          checked={settings.reroute_audio}
          onChange={(value) => set("reroute_audio", value)}
        />
        <Toggle
          id={`${ids}-fps-custom`}
          label={t`Use a frame rate of its own`}
          checked={settings.fps !== null}
          onChange={(value) => set("fps", value ? 30 : null)}
        />
        {settings.fps !== null && (
          <NumberField
            id={`${ids}-fps`}
            label={t`Frame rate`}
            value={settings.fps ?? 30}
            min={1}
            max={60}
            onChange={(value) => set("fps", value)}
          />
        )}
        <Field id={`${ids}-css`} label={t`Custom CSS`}>
          <textarea
            id={`${ids}-css`}
            value={settings.css}
            onChange={(event) => set("css", event.target.value)}
            rows={4}
            spellCheck={false}
            className="w-full rounded-md border border-input bg-transparent px-3 py-2 font-mono text-xs shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 dark:bg-input/30"
          />
        </Field>
      </MoreSettings>
    </>
  );
}
