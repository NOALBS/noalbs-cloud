import { plural, t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { type UseQueryResult, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { MonitorPlay, RotateCw } from "lucide-react";
import { Fragment, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { obsScenesQuery, queryKeys, sceneRulesQuery, streamServersQuery } from "@/api/queries";
import type { ObsScenes, SceneRule, Scenes, StreamServer, Switcher } from "@/api/types";
import { Section } from "@/components/page";
import { ScenePicker } from "@/components/scene-picker";
import { SceneRulesSection } from "@/components/scene-rules-section";
import { SaveBar } from "@/components/settings";
import { SwitcherBody } from "@/components/switcher-page";
import { Badge } from "@/components/ui/badge";
import { useNow } from "@/hooks/use-now";
import { useSwitcher } from "@/hooks/use-switcher";
import { errorMessage } from "@/lib/errors";
import { SCENE_ROLES } from "@/lib/scene-roles";
import { ruleLabel } from "@/lib/scene-rules";
import { formatRelative } from "@/lib/time";
import { cn } from "@/lib/utils";

interface RoleRow {
  key: keyof Scenes;
  label: string;
  description: string;
  dot: string;
  command?: string;
  /** The label in the OBS scene list, if shorter. */
  short?: string;
}

const automatic = (): RoleRow[] => [
  { key: "live", label: "LIVE", description: t`Your feed is healthy.`, dot: SCENE_ROLES.live.dot },
  {
    key: "low",
    label: "LOW",
    description: t`Bitrate or RTT crossed the LOW trigger.`,
    dot: SCENE_ROLES.low.dot,
  },
  {
    key: "offline",
    label: t`BRB / offline`,
    short: "BRB",
    description: t`No usable feed on any server.`,
    dot: SCENE_ROLES.offline.dot,
  },
];

const onCommand = (): RoleRow[] => [
  {
    key: "starting",
    label: t`Starting`,
    description: t`Before the stream goes live.`,
    dot: "bg-primary",
    command: "starting",
  },
  {
    key: "ending",
    label: t`Ending`,
    description: t`Wrapping up the stream.`,
    dot: "bg-primary",
    command: "ending",
  },
  {
    key: "privacy",
    label: t`Privacy`,
    description: t`Hide the camera instantly.`,
    dot: "bg-primary",
    command: "privacy",
  },
  {
    key: "refresh",
    label: t`Refresh`,
    description: t`Shown briefly while the feed reloads.`,
    dot: "bg-primary",
    command: "refresh",
  },
];

const same = (a: Scenes, b: Scenes) => JSON.stringify(a) === JSON.stringify(b);

export function ScenesPage() {
  const { workspace, switcher, canManage, path } = useSwitcher();
  const queryClient = useQueryClient();
  const obs = useQuery(obsScenesQuery(workspace.id, switcher.id));
  const servers = useQuery(streamServersQuery(workspace.id, switcher.id));
  const rules = useQuery(sceneRulesQuery(workspace.id, switcher.id));
  const [form, setForm] = useState<Scenes>(switcher.scenes);
  const [base, setBase] = useState<Scenes>(switcher.scenes);

  // Adopt fresh server data while nothing is being edited here.
  if (!same(switcher.scenes, base) && same(form, base)) {
    setBase(switcher.scenes);
    setForm(switcher.scenes);
  }

  const save = useMutation({
    mutationFn: () =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/scenes", {
          params: { path },
          body: form,
        }),
      ),
    onSuccess: (updated: Switcher) => {
      queryClient.setQueryData(queryKeys.switcher(workspace.id, switcher.id), updated);
      setBase(updated.scenes);
      setForm(updated.scenes);
      toast.success(t`Saved`);
    },
  });

  const prefix = switcher.chat.command_prefix;
  const editable = canManage && obs.isSuccess;

  return (
    <SwitcherBody
      title={t`Scenes`}
      description={t`Tell NOALBS which OBS scene to use for each situation.`}
      backToSettings
      actions={<SyncedChip obs={obs} />}
    >
      {obs.isError && (
        <div className="flex gap-3 rounded-2xl border border-dashed p-4">
          <MonitorPlay className="mt-0.5 size-5 shrink-0 text-primary" />
          <p className="text-sm text-muted-foreground">
            <Trans>
              {errorMessage(obs.error)} Scenes are picked from your OBS, so connect it to change
              them.
            </Trans>
          </p>
        </div>
      )}

      <div className="grid grid-cols-1 items-start gap-4 lg:grid-cols-12">
        <div className="flex flex-col gap-4 lg:col-span-8">
          {[
            {
              title: t`Automatic`,
              description: t`Chosen by the switcher from your bitrate and latency.`,
              rows: automatic(),
            },
            {
              title: t`On command`,
              description: t`Only used when you or a mod asks for them.`,
              rows: onCommand(),
            },
          ].map((group, index) => (
            <Fragment key={group.title}>
              {index === 1 && <SceneRulesSection obs={obs.data} servers={servers.data ?? []} />}
              <Section title={group.title} description={group.description}>
                <ul>
                  {group.rows.map((row) => (
                    <li
                      key={row.key}
                      className="flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-divider px-5 py-3"
                    >
                      <span
                        className={cn("h-9 w-2.5 shrink-0 rounded", row.dot)}
                        aria-hidden="true"
                      />
                      <span className="flex min-w-40 flex-1 flex-col gap-0.5">
                        <span className="text-sm font-semibold">{row.label}</span>
                        <span className="text-sm text-muted-foreground">{row.description}</span>
                      </span>
                      {row.command && (
                        <Badge variant="secondary" className="hidden font-mono sm:inline-flex">
                          {prefix}
                          {row.command}
                        </Badge>
                      )}
                      <ScenePicker
                        label={row.label}
                        value={form[row.key] ?? null}
                        obs={obs.data}
                        disabled={!editable}
                        onChange={(uuid) => setForm({ ...form, [row.key]: uuid })}
                      />
                    </li>
                  ))}
                </ul>
              </Section>
            </Fragment>
          ))}
        </div>

        <div className="flex flex-col gap-4 lg:col-span-4">
          <ObsSceneList
            obs={obs.data}
            scenes={form}
            servers={servers.data ?? []}
            rules={rules.data ?? []}
          />
          <OverridesNote servers={servers.data ?? []} obs={obs.data} />
        </div>
      </div>

      <SaveBar
        dirty={!same(form, base)}
        saving={save.isPending}
        error={save.error ? errorMessage(save.error) : null}
        onSave={() => save.mutate()}
        onDiscard={() => {
          setForm(base);
          save.reset();
        }}
      />
    </SwitcherBody>
  );
}

/** When the scene list was last read from OBS, with a way to read it again. */
function SyncedChip({ obs }: { obs: UseQueryResult<ObsScenes> }) {
  const now = useNow();
  if (!obs.isSuccess) return null;

  return (
    <button
      type="button"
      onClick={() => void obs.refetch()}
      className="flex h-9 items-center gap-2 rounded-full border px-3 text-[13px] text-muted-foreground hover:text-foreground"
    >
      <RotateCw className={cn("size-3.5", obs.isFetching && "animate-spin")} />
      <Trans>Synced from OBS {formatRelative(new Date(obs.dataUpdatedAt), new Date(now))}</Trans>
    </button>
  );
}

/** Every scene in OBS, and what NOALBS uses it for. */
function ObsSceneList({
  obs,
  scenes,
  servers,
  rules,
}: {
  obs: ObsScenes | undefined;
  scenes: Scenes;
  servers: StreamServer[];
  rules: SceneRule[];
}) {
  if (!obs) return null;

  const usesOf = (uuid: string | null | undefined) => {
    if (!uuid) return [];
    const roles = [...automatic(), ...onCommand()]
      .filter((row) => scenes[row.key] === uuid)
      .map((row) => row.short ?? row.label);
    const overrides = servers
      .filter((server) => Object.values(server.overrides).includes(uuid))
      .map((server) => t`Override · ${server.name}`);
    const combinations = rules
      .filter((rule) => Object.values(rule.scenes).includes(uuid))
      .map((rule) => t`Combination · ${ruleLabel(rule, servers)}`);
    return [...roles, ...overrides, ...combinations];
  };

  return (
    <section aria-label={t`Scenes in OBS`} className="flex flex-col rounded-2xl border bg-card">
      <div className="flex items-baseline justify-between px-5 pt-5 pb-3">
        <h2 className="font-semibold">
          <Trans>Scenes in OBS</Trans>
        </h2>
        <span className="font-mono text-xs text-muted-foreground">
          {plural(obs.scenes.length, { one: "# scene", other: "# scenes" })}
        </span>
      </div>
      <ul>
        {obs.scenes.map((scene) => {
          const onAir = scene.uuid != null && scene.uuid === obs.current_scene_uuid;
          const uses = usesOf(scene.uuid);
          return (
            <li
              key={scene.uuid ?? scene.name}
              className="flex min-h-11 flex-wrap items-center justify-between gap-x-3 gap-y-0.5 border-t border-divider px-5 py-2"
            >
              <span className="flex max-w-full min-w-0 items-center gap-2 text-sm font-medium">
                {onAir && (
                  <span className="size-2 shrink-0 rounded-full bg-offline">
                    <span className="sr-only">
                      <Trans>On air</Trans>
                    </span>
                  </span>
                )}
                <span className="truncate">{scene.name}</span>
              </span>
              <span className="ml-auto text-right text-xs text-muted-foreground">
                {uses.length ? uses.join(" · ") : "—"}
              </span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/** Which servers swap in their own scenes, and where to change that. */
function OverridesNote({ servers, obs }: { servers: StreamServer[]; obs: ObsScenes | undefined }) {
  const { workspace, switcher } = useSwitcher();
  const nameOf = (uuid: string) =>
    obs?.scenes.find((scene) => scene.uuid === uuid)?.name ?? t`another scene`;
  const lines = servers.flatMap((server) =>
    (["live", "low", "offline"] as const).flatMap((role) => {
      const uuid = server.overrides[role];
      return uuid ? [{ server: server.name, scene: nameOf(uuid), role }] : [];
    }),
  );

  return (
    <section className="flex flex-col gap-2 rounded-2xl border border-dashed p-5">
      <span className="text-sm font-semibold">
        <Trans>Per-server overrides</Trans>
      </span>
      <span className="text-[13px] leading-relaxed text-muted-foreground">
        {lines.length === 0
          ? t`No server swaps in its own scenes. `
          : lines.map((line) => (
              <span key={`${line.server}-${line.role}`} className="block">
                <Trans>
                  {line.server} uses <span className="text-foreground">{line.scene}</span> instead
                  of {SCENE_ROLES[line.role].label}.
                </Trans>
              </span>
            ))}
        <Trans>Set overrides on each stream server.</Trans>
      </span>
      <Link
        to="/$workspace/s/$switcher/switching"
        hash="servers"
        params={{ workspace: workspace.url_slug, switcher: switcher.id }}
        className="text-sm font-medium text-primary hover:underline"
      >
        <Trans>Open stream servers</Trans>
      </Link>
    </section>
  );
}
