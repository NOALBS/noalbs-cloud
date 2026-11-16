import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { MonitorPlay, PauseCircle, PlugZap, WifiOff } from "lucide-react";
import { type ComponentProps, type ReactNode, useState } from "react";
import {
  streamMarkersQuery,
  streamServersQuery,
  streamsQuery,
  switchersQuery,
} from "@/api/queries";
import type { SwitcherStatus } from "@/api/types";
import { ConnectDialog } from "@/components/connect-controls";
import { ActivityFeed } from "@/components/live/activity-feed";
import { AudioMixer } from "@/components/live/audio-mixer";
import { AutoSwitchPane } from "@/components/live/auto-switch-pane";
import { BelaboxCard } from "@/components/live/belabox-card";
import { BitrateChart } from "@/components/live/bitrate-chart";
import { lookFor } from "@/components/live/feed-state";
import { useLiveVariants, VariantPicker } from "@/components/live/live-variants";
import { MoblinCard } from "@/components/live/moblin-card";
import { ObsPreview } from "@/components/live/obs-preview";
import { ObsStatsPopover } from "@/components/live/obs-stats-popover";
import { OperatorHeader } from "@/components/live/operator-strip";
import { PaneLayout } from "@/components/live/pane-layout";
import { ProgramPreview } from "@/components/live/program-preview";
import { useOperator } from "@/components/live/use-operator";
import "@/components/live/operator.css";
import {
  AutoSwitchToggle,
  FixButton,
  MarkerButton,
  type ObsControls,
  RecordButton,
  StreamButton,
  useObsControls,
} from "@/components/live/quick-actions";
import { SceneGrid } from "@/components/live/scene-grid";
import { ServerCard } from "@/components/live/server-card";
import { SourceList } from "@/components/live/source-list";
import { SwitcherPage } from "@/components/switcher-page";
import { Button } from "@/components/ui/button";
import { useConnectObsCommand } from "@/hooks/use-connect-obs-command";
import { useNow } from "@/hooks/use-now";
import { useSwitcher } from "@/hooks/use-switcher";
import {
  canControlConnect,
  canManageConnectPrograms,
  connectObsActionAvailability,
  pcObsLabel,
} from "@/lib/connect";
import { formatKbps, formatUptime } from "@/lib/live-format";
import { can, isOwner, isSupport, seesPreviewOffAir } from "@/lib/permissions";
import { cn } from "@/lib/utils";
import { isStale, latest, type Point } from "@/live/reducer";
import { useSwitcherLive } from "@/live/use-switcher-live";

export const Route = createFileRoute("/_app/$workspace/s/$switcher/live")({
  component: LivePage,
});

function LivePage() {
  const { workspace, switcher, actor, path } = useSwitcher();
  const live = useSwitcherLive(workspace.id, switcher.id);
  const servers = useQuery(streamServersQuery(workspace.id, switcher.id));
  const summaries = useQuery(switchersQuery(workspace.id));
  const controls = useObsControls(path);
  const obsCommand = useConnectObsCommand(path);
  const now = useNow();
  const canControl = can(actor, "switcher.control");
  const canControlConnectObs = canControlConnect(switcher.connect_obs_control, actor);

  const status =
    live.status && !isStale(live.status, now)
      ? live.status
      : live.status === undefined
        ? undefined
        : null;
  const connectDeviceName = status?.pc?.device_name ?? t`Connect computer`;
  const activeServer =
    servers.data?.find((server) => server.id === status?.active_server_id) ?? servers.data?.[0];
  const sample = activeServer && status?.obs_connected ? latest(live, activeServer.id) : undefined;
  const params = { workspace: workspace.url_slug, switcher: switcher.id };
  const paused = Boolean(
    summaries.data?.find((summary) => summary.id === switcher.id)?.paused || status?.paused,
  );
  const disabled = !canControl || !status?.obs_connected || paused;
  // The dashboard lost its own connection (often the phone's signal): what's shown is
  // out of date, and says nothing about OBS or the browser source.
  const feedDown = live.connection === "reconnecting";
  const [layoutHeader, setLayoutHeader] = useState<HTMLDivElement | null>(null);
  const operator = useOperator(path, status, disabled, live.events);
  const variants = useLiveVariants();
  const { studio, preview: previewScene } = operator;
  const onAir = status?.current_scene;

  return (
    <div
      ref={operator.root}
      className="live-operator-root"
      data-immersive={operator.immersive}
      data-operator={operator.enabled}
      data-variant={variants.variant}
    >
      <SwitcherPage
        className="live-operator-page max-w-none"
        title={t`Live`}
        description={`${switcher.name} · ${activeServer ? activeServer.name : t`No stream server`} → OBS`}
        actions={
          <div className="flex flex-wrap items-center gap-2.5">
            <OperatorHeader
              operator={operator}
              status={status}
              controls={controls}
              disabled={disabled}
              now={now}
            />
            <div ref={setLayoutHeader} />
            {status?.pc ? (
              <ConnectDialog
                path={path}
                deviceName={connectDeviceName}
                obs={status.pc.obs}
                streaming={status.streaming}
                canControlObs={canControlConnectObs}
                canControlPrograms={canManageConnectPrograms(switcher.connect_programs, actor)}
                settingsLink={
                  <Link
                    to="/$workspace/s/$switcher/obs"
                    params={params}
                    className="font-semibold text-primary hover:underline"
                  >
                    <Trans>Connect settings</Trans>
                  </Link>
                }
              >
                <Health
                  ok={Boolean(status.browser_source)}
                  label={t`Connect · ${connectDeviceName}`}
                  title={t`Connect controls`}
                  interactive
                />
              </ConnectDialog>
            ) : (
              <Health ok={Boolean(status?.browser_source)} label={t`Browser source`} />
            )}
            <ObsStatsPopover
              path={path}
              connected={Boolean(status?.obs_connected)}
              connectDevice={status?.pc?.device_name ?? undefined}
            >
              <Health ok={Boolean(status?.obs_connected)} label="OBS" interactive />
            </ObsStatsPopover>
            {live.connection === "reconnecting" && <Health ok={false} label={t`Live feed`} />}
          </div>
        }
      >
        {operator.immersive && (
          <Button
            className="immersive-exit self-end"
            variant="outline"
            size="sm"
            onClick={operator.toggleImmersive}
          >
            Exit immersive · Esc / F
          </Button>
        )}
        {!operator.enabled && <VariantPicker model={variants} />}
        {paused && workspace.suspension && (
          <div className="flex flex-wrap items-center gap-3 rounded-2xl border border-destructive/40 bg-destructive/10 p-4">
            <PauseCircle className="size-5 text-destructive-foreground" />
            <p className="flex-1 text-sm">
              {workspace.suspension.reason ? (
                <>
                  <Trans>
                    <strong>Suspended by NOALBS.</strong> {workspace.suspension.reason} This
                    switcher stays connected but doesn't switch or take commands. Questions:{" "}
                    <a
                      href="mailto:support@noalbs.com"
                      className="font-semibold text-primary hover:underline"
                    >
                      support@noalbs.com
                    </a>
                  </Trans>
                </>
              ) : (
                <>
                  <Trans>
                    <strong>This workspace is paused.</strong> This switcher stays connected but
                    doesn't switch or take commands.
                  </Trans>
                  {workspace.managed_by && (
                    <>
                      {" "}
                      <Trans>Ask {workspace.managed_by.name} about it.</Trans>
                    </>
                  )}
                </>
              )}
            </p>
          </div>
        )}
        {paused && !workspace.suspension && (
          <div className="flex flex-wrap items-center gap-3 rounded-2xl border border-low/40 bg-low/10 p-4">
            <PauseCircle className="size-5 text-low" />
            <p className="flex-1 text-sm">
              <Trans>
                <strong>Paused.</strong> This workspace has more switchers than its plan includes,
                so this one stays connected but doesn't switch or take commands. Upgrade the plan,
                or delete a switcher you no longer use.
              </Trans>
            </p>
            <Link
              to="/$workspace/settings/plan"
              params={{ workspace: workspace.url_slug }}
              className="text-sm font-semibold text-primary hover:underline"
            >
              <Trans>See plans</Trans>
            </Link>
          </div>
        )}

        {feedDown && (
          <div
            role="status"
            className="flex flex-wrap items-center gap-3 rounded-2xl border border-low/40 bg-low/10 p-4"
          >
            <WifiOff className="size-5 text-low" />
            <p className="flex-1 text-sm">
              <Trans>
                <strong>Reconnecting to NOALBS…</strong> This page is out of date until it’s back.
                Switching keeps running without it; check your phone’s signal.
              </Trans>
            </p>
          </div>
        )}

        {status === null && !feedDown && (
          <div className="flex flex-wrap items-center gap-3 rounded-2xl border border-dashed p-4">
            <MonitorPlay className="size-5 text-primary" />
            <p className="flex-1 text-sm text-muted-foreground">
              <Trans>
                The browser source isn’t connected. Open OBS with the NOALBS browser source active.
              </Trans>
            </p>
            <Link
              to="/$workspace/s/$switcher/setup"
              params={params}
              className="text-sm font-semibold text-primary hover:underline"
            >
              <Trans>Setup help</Trans>
            </Link>
          </div>
        )}

        {status && !status.obs_connected && !feedDown && (
          // Text gets the full width on phones, with the actions below it (lined up
          // with the text); side by side from `sm`. The device name is the user's own.
          <div className="flex flex-col gap-3 rounded-2xl border border-low/40 bg-low/10 p-4 sm:flex-row sm:items-center">
            <div className="flex min-w-0 flex-1 items-start gap-3">
              <PlugZap className="mt-px size-5 shrink-0 text-low" />
              <p className="min-w-0 flex-1 text-sm [overflow-wrap:anywhere]">
                {status.pc ? (
                  <Trans>
                    <strong>{connectDeviceName} is connected, but OBS isn’t.</strong>{" "}
                    {pcObsLabel(status.pc.obs)}.
                  </Trans>
                ) : (
                  <Trans>
                    <strong>The browser source can’t reach OBS.</strong> Check that WebSocket is on
                    in OBS (Tools → WebSocket Server Settings) and that the port and password match
                    Setup.
                  </Trans>
                )}
              </p>
            </div>
            <div className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2 pl-8 sm:pl-0">
              {status.pc ? (
                <>
                  {canControlConnectObs && connectObsActionAvailability(status.pc.obs).canStart && (
                    <Button
                      size="sm"
                      disabled={obsCommand.isPending}
                      onClick={() => obsCommand.mutate("start")}
                    >
                      <Trans>Start OBS</Trans>
                    </Button>
                  )}
                  <Link
                    to="/$workspace/s/$switcher/obs"
                    params={params}
                    className="text-sm font-semibold text-primary hover:underline"
                  >
                    <Trans>Open settings</Trans>
                  </Link>
                </>
              ) : (
                <Link
                  to="/$workspace/s/$switcher/setup"
                  params={params}
                  className="text-sm font-semibold text-primary hover:underline"
                >
                  <Trans>Open Setup</Trans>
                </Link>
              )}
            </div>
          </div>
        )}

        <PaneLayout
          switcherId={switcher.id}
          operatorMode={operator.enabled}
          variant={operator.enabled ? "c" : variants.variant}
          designs={
            operator.enabled
              ? { scenes: "b", actions: "c", servers: "c", activity: "b", monitors: "c" }
              : variants.designs
          }
          header={layoutHeader}
          panes={{
            "auto-switch": (
              <AutoSwitchPane
                controls={controls}
                status={status}
                enabled={switcher.behaviour.bitrate_switcher_enabled}
                onlyWhileStreaming={switcher.behaviour.only_switch_when_streaming}
                canControl={canControl && !disabled}
                triggers={switcher.triggers}
                watching={activeServer?.name}
                points={activeServer ? (live.history[activeServer.id] ?? []) : []}
                now={now}
              />
            ),
            status: (
              <StatusCard
                status={status && onAir ? { ...status, current_scene: onAir } : status}
                now={now}
                sample={sample}
                serverName={activeServer?.name}
                serverInUse={Boolean(activeServer && activeServer.id === status?.active_server_id)}
                paused={paused}
                feedDown={feedDown}
              />
            ),
            controls: (
              <MobileActions
                path={path}
                controls={controls}
                status={status}
                disabled={disabled}
                canControl={canControl}
                autoSwitchSetting={switcher.behaviour.bitrate_switcher_enabled}
                onlyWhileStreaming={switcher.behaviour.only_switch_when_streaming}
              />
            ),
            "bitrate-chart": (
              <section
                aria-label={t`Bitrate`}
                className={cn("flex flex-col gap-3", feedDown && "opacity-50")}
              >
                <div className="flex flex-wrap items-end justify-between gap-4">
                  <div className="flex items-end gap-8">
                    <Stat label={t`Bitrate`}>
                      <span className="text-5xl">{sample ? formatKbps(sample.bitrate) : "—"}</span>
                      <span className="text-base font-normal text-muted-foreground">
                        {" "}
                        <Trans>kbps</Trans>
                      </span>
                    </Stat>
                    <Stat label="RTT">
                      <span className="text-2xl">
                        {sample?.rtt != null ? Math.round(sample.rtt) : "—"}
                      </span>
                      <span className="text-sm font-normal text-muted-foreground">
                        {" "}
                        <Trans>ms</Trans>
                      </span>
                    </Stat>
                  </div>
                  <div className="flex flex-wrap gap-3 text-xs text-muted-foreground">
                    {switcher.triggers.low != null && (
                      <span className="flex items-center gap-1.5">
                        <span className="w-3.5 border-t border-dashed border-low" />
                        <Trans>Low {formatKbps(switcher.triggers.low)}</Trans>
                      </span>
                    )}
                    {switcher.triggers.offline != null && (
                      <span className="flex items-center gap-1.5">
                        <span className="w-3.5 border-t border-dashed border-offline" />
                        <Trans>BRB {formatKbps(switcher.triggers.offline)}</Trans>
                      </span>
                    )}
                    <span>
                      <Trans>Last 2 min</Trans>
                    </span>
                  </div>
                </div>
                <BitrateChart
                  points={activeServer ? (live.history[activeServer.id] ?? []) : []}
                  now={now}
                  low={switcher.triggers.low}
                  offline={switcher.triggers.offline}
                  tone={lookFor(status).text}
                />
              </section>
            ),
            servers: (
              <section aria-label={t`Stream servers`} className="flex flex-col">
                <div className="flex items-baseline justify-between">
                  <h2 className="font-semibold">
                    <Trans>Stream servers</Trans>
                  </h2>
                  <Link
                    to="/$workspace/s/$switcher/servers"
                    params={params}
                    className="text-sm text-primary hover:underline"
                  >
                    <Trans>Manage</Trans>
                  </Link>
                </div>
                {servers.data?.length === 0 && (
                  <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
                    <Trans>No stream servers yet.</Trans>
                  </p>
                )}
                {(operator.enabled
                  ? [...(servers.data ?? [])].sort((a, b) => a.priority - b.priority)
                  : servers.data
                )?.map((server, index) => (
                  <ServerCard
                    key={server.id}
                    server={server}
                    points={live.history[server.id] ?? []}
                    now={now}
                    inUse={server.id === status?.active_server_id}
                    rank={operator.enabled ? index + 1 : undefined}
                  />
                ))}
              </section>
            ),
            belabox: switcher.belabox.has_remote_key ? (
              <BelaboxCard
                status={live.belabox}
                history={live.belaboxHistory}
                now={now}
                path={path}
                canControl={canControl && !paused}
                canConfigure={canControl && !paused && !isSupport(actor)}
                canManage={can(actor, "switcher.manage") && !paused}
                labels={switcher.belabox.interface_labels}
                settingsLink={params}
              />
            ) : (
              <p className="p-4 text-muted-foreground">BELABOX is not configured.</p>
            ),
            moblin: switcher.moblin.has_password ? (
              <MoblinCard
                status={live.moblin}
                history={live.moblinHistory}
                now={now}
                path={path}
                canControl={canControl && !paused}
              />
            ) : (
              <p className="p-4 text-muted-foreground">Moblin is not configured.</p>
            ),
            preview: (
              <ObsPreview
                path={path}
                obsConnected={Boolean(status?.obs_connected)}
                streaming={Boolean(status?.streaming)}
                offAirAllowed={seesPreviewOffAir(actor, switcher.preview_off_air)}
                privacyLink={isOwner(actor) ? params : undefined}
              />
            ),
            scenes: (
              <SceneGrid
                path={path}
                scenes={switcher.scenes}
                currentUuid={onAir?.uuid}
                previewUuid={studio ? previewScene?.uuid : undefined}
                onPreviewSelect={studio ? operator.cue : undefined}
                onProgramSelect={operator.cutScene}
                operatorTools={
                  operator.enabled ? (
                    <span className="operator-scene-tools">
                      <FixButton controls={controls} disabled={disabled} variant="compact" />
                      <MarkerButton
                        controls={controls}
                        status={status}
                        disabled={disabled}
                        variant="compact"
                      />
                      <RecordButton
                        controls={controls}
                        status={status}
                        disabled={disabled}
                        variant="compact"
                      />
                    </span>
                  ) : undefined
                }
                canControl={canControl && !paused}
                canManage={can(actor, "switcher.manage") && !paused && !isSupport(actor)}
                obsConnected={Boolean(status?.obs_connected)}
              />
            ),
            sources: (
              <SourceList
                path={path}
                scene={status?.current_scene?.uuid ?? status?.current_scene?.name ?? null}
                programName={status?.streaming ? status.current_scene?.name : null}
                previewName={studio ? previewScene?.name : undefined}
                canControl={canControl && !paused}
                canEditSources={can(actor, "switcher.manage") && !paused && !isSupport(actor)}
                obsConnected={Boolean(status?.obs_connected)}
              />
            ),
            audio: (
              <AudioMixer
                path={path}
                scene={status?.current_scene?.uuid ?? status?.current_scene?.name ?? null}
                canControl={canControl && !paused}
                obsConnected={Boolean(status?.obs_connected)}
              />
            ),
            activity: (
              <ActivityFeed
                workspaceId={workspace.id}
                switcherId={switcher.id}
                live={live.events}
              />
            ),
            "program-preview": (
              <ProgramPreview
                operator={operator}
                path={path}
                connected={Boolean(status?.obs_connected)}
                streaming={Boolean(status?.streaming)}
                offAirAllowed={seesPreviewOffAir(actor, switcher.preview_off_air)}
              />
            ),
          }}
        />
      </SwitcherPage>
    </div>
  );
}

/** Stream, record, auto switch, fix and marker as a grid, on phones. */
function MobileActions({
  path,
  controls,
  status,
  disabled,
  canControl,
  autoSwitchSetting,
  onlyWhileStreaming,
}: {
  path: { workspace_id: string; switcher_id: string };
  controls: ObsControls;
  status: SwitcherStatus | null | undefined;
  disabled: boolean;
  canControl: boolean;
  autoSwitchSetting: boolean;
  onlyWhileStreaming: boolean;
}) {
  const streams = useInfiniteQuery({
    ...streamsQuery(path.workspace_id, path.switcher_id),
    enabled: Boolean(status?.streaming),
    refetchInterval: 30_000,
  });
  const active = status?.streaming
    ? streams.data?.pages.flat().find((stream) => !stream.ended_at)
    : undefined;
  const markers = useQuery({
    ...streamMarkersQuery(path.workspace_id, path.switcher_id, active?.id ?? ""),
    enabled: Boolean(active),
    refetchInterval: 15_000,
  });
  return (
    <section aria-label={t`Actions`} className="grid grid-cols-2 gap-2">
      <StreamButton controls={controls} status={status} disabled={disabled} variant="tile" />
      <RecordButton controls={controls} status={status} disabled={disabled} variant="tile" />
      <FixButton controls={controls} disabled={disabled} variant="tile" />
      <MarkerButton
        controls={controls}
        status={status}
        disabled={disabled}
        variant="tile"
        markerCount={status?.streaming ? markers.data?.length : 0}
      />
      <AutoSwitchToggle
        controls={controls}
        status={status}
        autoSwitchSetting={autoSwitchSetting}
        onlyWhileStreaming={onlyWhileStreaming}
        canControl={canControl}
        className="col-span-2 mt-1 rounded-none border-0 border-t bg-transparent px-0"
      />
    </section>
  );
}

function Stat({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-xs tracking-wider text-muted-foreground uppercase">{label}</span>
      <span className="font-mono leading-none font-semibold tabular">{children}</span>
    </div>
  );
}

function StatusCard({
  status,
  now,
  sample,
  serverName,
  serverInUse,
  paused,
  feedDown,
  className,
}: {
  status: SwitcherStatus | null | undefined;
  now: number;
  sample: Point | undefined;
  serverName: string | undefined;
  serverInUse: boolean;
  paused: boolean;
  /** The dashboard's own connection dropped: show the last state, greyed out. */
  feedDown: boolean;
  className?: string;
}) {
  const look = feedDown ? lookFor(undefined) : lookFor(status);

  return (
    <section aria-label={t`Stream status`} className={cn("flex flex-col gap-2", className)}>
      <div className="flex items-center justify-between gap-3">
        <span
          className={cn(
            "flex items-center gap-2 text-xs font-semibold tracking-wider uppercase",
            status?.streaming ? "text-offline" : "text-muted-foreground",
          )}
          role="status"
        >
          <span
            className={cn(
              "size-1.5 rounded-full",
              status?.streaming ? "bg-offline" : "bg-muted-foreground",
            )}
          />
          {feedDown
            ? t`Reconnecting…`
            : status === undefined
              ? t`Connecting…`
              : paused
                ? t`Paused`
                : look.label}
        </span>
        <span className="font-mono text-xs text-muted-foreground tabular">
          {status?.streaming && status.stream_started_at
            ? formatUptime(status.stream_started_at, now)
            : "--:--:--"}
        </span>
      </div>

      {status?.obs_connected && status.streaming && status.state !== "offline" ? (
        <div className={cn(feedDown && "opacity-50")}>
          <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1 font-mono tabular">
            <span className="text-5xl leading-none font-bold">
              {sample ? formatKbps(sample.bitrate) : "—"}
            </span>
            <span className="text-xs text-muted-foreground">
              <Trans>kbps</Trans>
            </span>
            <span className="ml-2 text-lg font-semibold">
              {sample?.rtt != null ? Math.round(sample.rtt) : "—"}
            </span>
            <span className="text-xs text-muted-foreground">
              <Trans>ms RTT</Trans>
            </span>
          </div>
          {serverName && (
            <p className="mt-1 text-xs text-muted-foreground">
              {serverName}
              {serverInUse && (
                <>
                  {" "}
                  · <Trans>in use</Trans>
                </>
              )}
            </p>
          )}
        </div>
      ) : (
        <div>
          <h3 className="text-2xl font-bold">
            {status === undefined
              ? t`Connecting…`
              : !status?.obs_connected
                ? t`OBS is offline`
                : !status.streaming
                  ? t`No feed yet`
                  : t`No feed`}
          </h3>
          <p className="mt-1 text-xs text-muted-foreground">
            {status?.obs_connected
              ? t`The bitrate shows here once a server receives video.`
              : t`NOALBS needs OBS open with the browser source active.`}
          </p>
        </div>
      )}
      <div className="space-y-1 border-t border-divider pt-2">
        <p className="text-xs tracking-wide text-muted-foreground uppercase">
          {status?.streaming ? t`On air` : t`In OBS`}
        </p>
        <p className="text-sm font-semibold">{status?.current_scene?.name ?? "—"}</p>
        {paused ? (
          <p className="text-xs text-muted-foreground">
            <Trans>Not switching while paused</Trans>
          </p>
        ) : (
          status &&
          !status.auto_switch && (
            <p className="text-xs text-muted-foreground">
              <Trans>Auto switch is off</Trans>
            </p>
          )
        )}
      </div>
    </section>
  );
}

/**
 * A connection chip. `interactive` makes it a button (a popover trigger passes
 * its props and ref through `rest`).
 */
function Health({
  ok,
  label,
  interactive,
  ...rest
}: { ok: boolean; label: string; interactive?: boolean } & ComponentProps<"button">) {
  const content = (
    <>
      <span className={cn("size-1.5 shrink-0 rounded-full", ok ? "bg-live" : "bg-offline")} />
      {/* Labels can carry a user-chosen computer name. */}
      <span className="max-w-56 truncate">{label}</span>
      <span className="sr-only">{ok ? t`connected` : t`not connected`}</span>
    </>
  );
  const className =
    "flex h-9 items-center gap-2 rounded-full border px-3 text-[13px] text-muted-foreground";
  if (!interactive) return <span className={className}>{content}</span>;
  return (
    <button
      type="button"
      title={t`${label} stats`}
      className={cn(
        className,
        "transition-colors hover:bg-accent hover:text-foreground data-[state=open]:bg-accent data-[state=open]:text-foreground",
      )}
      {...rest}
    >
      {content}
    </button>
  );
}
