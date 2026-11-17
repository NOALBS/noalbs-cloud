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
import { BelaboxCard } from "@/components/live/belabox-card";
import { BitratePane } from "@/components/live/bitrate-pane";
import {
  LiveHeaderAnchor,
  LiveHeaderControls,
  LiveTimer,
  RecordingStatus,
} from "@/components/live/live-header";
import { streamState } from "@/components/live/live-status";
import { MoblinCard } from "@/components/live/moblin-card";
import { Monitors } from "@/components/live/monitors";
import { ObsPane } from "@/components/live/obs-pane";
import { ObsPreview } from "@/components/live/obs-preview";
import { OnAirPane } from "@/components/live/on-air-pane";
import { Pane } from "@/components/live/pane";
import { PaneLayout } from "@/components/live/pane-layout";
import { PaneTitleRow } from "@/components/live/pane-title-row";
import {
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
import { useOperator } from "@/components/live/use-operator";
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
import { can, isOwner, isSupport, seesPreviewOffAir } from "@/lib/permissions";
import { cn } from "@/lib/utils";
import { isStale, latest } from "@/live/reducer";
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

  // The dashboard lost its own connection: OBS may still be streaming.
  const feedDown = live.connection === "reconnecting";
  const status =
    !feedDown &&
    live.status &&
    Number.isFinite(Date.parse(live.status.updated_at)) &&
    !isStale(live.status, now)
      ? live.status
      : live.status === undefined
        ? undefined
        : null;
  const obsConnected = streamState(status, now) !== "unknown";
  const connectDeviceName = status?.pc?.device_name ?? t`Connect computer`;
  const activeServer =
    servers.data?.find((server) => server.id === status?.active_server_id) ?? servers.data?.[0];
  const sample = activeServer && obsConnected ? latest(live, activeServer.id) : undefined;
  const params = { workspace: workspace.url_slug, switcher: switcher.id };
  const paused = Boolean(
    summaries.data?.find((summary) => summary.id === switcher.id)?.paused || status?.paused,
  );
  const disabled = !canControl || !obsConnected || paused;
  const [layoutHeader, setLayoutHeader] = useState<HTMLDivElement | null>(null);
  const operator = useOperator(path, status, disabled, live.events, live.status?.current_scene);
  const { studio, preview: previewScene } = operator;
  const onAir = status?.current_scene;
  const [devicesTab, setDevicesTab] = useState<"belabox" | "moblin">("belabox");

  return (
    <div ref={operator.root} className="live-root" data-immersive={operator.immersive}>
      <LiveHeaderAnchor top={operator.desktop || operator.immersive ? 0 : 60} />
      <SwitcherPage
        className="live-page max-w-none"
        title={
          <span className="flex flex-wrap items-center gap-2 md:gap-3">
            <span className="live-title text-[17px] font-semibold tracking-tight">
              {switcher.name}
              <span className="mx-1.5 font-normal text-subtle-foreground">/</span>
              <Trans>Live</Trans>
            </span>
            {/* On desktop the stream and record buttons carry the timers. */}
            {(!operator.desktop || streamState(status, now) === "unknown") && (
              <>
                <LiveTimer status={status} now={now} />
                <RecordingStatus status={status} now={now} />
              </>
            )}
          </span>
        }
        actions={
          <div className="live-header-bar">
            {status?.pc ? (
              <span className="contents max-md:hidden">
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
                    shortLabel={t`Connect`}
                    title={t`Connect · ${connectDeviceName}`}
                    interactive
                  />
                </ConnectDialog>
              </span>
            ) : (
              <span className="contents max-md:hidden">
                <Health
                  ok={Boolean(status?.browser_source)}
                  label={t`Browser source`}
                  shortLabel={t`Source`}
                />
              </span>
            )}
            {live.connection === "reconnecting" && <Health ok={false} label={t`Live feed`} />}
            <LiveHeaderControls operator={operator} />
            <div ref={setLayoutHeader} className="contents" />
            {operator.desktop && (
              <div className="live-header-transport">
                <RecordButton
                  controls={controls}
                  status={status}
                  disabled={disabled}
                  variant="compact"
                />
                <StreamButton
                  controls={controls}
                  status={status}
                  disabled={disabled}
                  variant="compact"
                />
              </div>
            )}
          </div>
        }
      >
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
          mode={operator.enabled ? "operator" : "streamer"}
          toolbar={layoutHeader}
          panes={{
            monitors: (
              <Monitors
                operator={operator}
                path={path}
                connected={obsConnected}
                streaming={Boolean(status?.streaming)}
                offAirAllowed={seesPreviewOffAir(actor, switcher.preview_off_air)}
              />
            ),
            "on-air": (
              <OnAirPane
                status={status}
                controls={controls}
                disabled={disabled}
                by={operator.by ?? undefined}
                paused={paused}
                lastKnownScene={operator.program?.name}
              />
            ),
            controls: (
              <MobileActions path={path} controls={controls} status={status} disabled={disabled} />
            ),
            bitrate: (
              <BitratePane
                controls={controls}
                status={status}
                enabled={switcher.behaviour.bitrate_switcher_enabled}
                onlyWhileStreaming={switcher.behaviour.only_switch_when_streaming}
                canControl={canControl && !paused}
                triggers={switcher.triggers}
                server={activeServer?.name}
                sample={feedDown ? undefined : sample}
                points={activeServer ? (live.history[activeServer.id] ?? []) : []}
                now={now}
              />
            ),
            obs: (
              <ObsPane
                path={path}
                connected={obsConnected}
                connect={
                  status?.pc ? { deviceName: connectDeviceName, obs: status.pc.obs } : undefined
                }
                streaming={Boolean(status?.streaming)}
                canControlObs={canControlConnectObs}
                canControlPrograms={canManageConnectPrograms(switcher.connect_programs, actor)}
              />
            ),
            servers: (
              <Pane
                title={<Trans>Stream servers</Trans>}
                label={t`Stream servers`}
                tools={
                  <Button asChild variant="outline" size="sm">
                    <Link to="/$workspace/s/$switcher/servers" params={params}>
                      <Trans>Manage</Trans>
                    </Link>
                  </Button>
                }
              >
                {servers.data?.length === 0 && (
                  <p className="text-sm text-muted-foreground">
                    <Trans>No stream servers yet.</Trans>
                  </p>
                )}
                {[...(servers.data ?? [])]
                  .sort((a, b) => a.priority - b.priority)
                  .map((server) => (
                    <ServerCard
                      key={server.id}
                      server={server}
                      points={live.history[server.id] ?? []}
                      now={now}
                      inUse={server.id === status?.active_server_id}
                    />
                  ))}
              </Pane>
            ),
            devices:
              switcher.belabox.has_remote_key || switcher.moblin.has_password ? (
                <DevicesPane
                  tab={
                    !switcher.moblin.has_password
                      ? "belabox"
                      : !switcher.belabox.has_remote_key
                        ? "moblin"
                        : devicesTab
                  }
                  onTab={
                    switcher.belabox.has_remote_key && switcher.moblin.has_password
                      ? setDevicesTab
                      : undefined
                  }
                  belabox={
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
                  }
                  moblin={
                    <MoblinCard
                      status={live.moblin}
                      history={live.moblinHistory}
                      now={now}
                      path={path}
                      canControl={canControl && !paused}
                    />
                  }
                />
              ) : null,
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
            ) : null,
            moblin: switcher.moblin.has_password ? (
              <MoblinCard
                status={live.moblin}
                history={live.moblinHistory}
                now={now}
                path={path}
                canControl={canControl && !paused}
              />
            ) : null,
            preview: (
              <ObsPreview
                path={path}
                obsConnected={obsConnected}
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
                lastKnownUuid={operator.program?.uuid}
                streaming={Boolean(status?.streaming)}
                previewUuid={studio ? previewScene?.uuid : undefined}
                onPreviewSelect={studio ? operator.cue : undefined}
                onProgramSelect={operator.cutScene}
                hotkeys={operator.enabled && operator.desktop}
                operatorTools={
                  operator.enabled ? (
                    <span className="mr-2 flex items-center gap-2">
                      <FixButton controls={controls} disabled={disabled} variant="compact" />
                      <MarkerButton
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
                obsConnected={obsConnected}
              />
            ),
            sources: (
              <SourceList
                path={path}
                scene={status?.current_scene?.uuid ?? status?.current_scene?.name ?? null}
                programName={obsConnected && status?.streaming ? status.current_scene?.name : null}
                previewName={studio ? previewScene?.name : undefined}
                canControl={canControl && !paused}
                canEditSources={can(actor, "switcher.manage") && !paused && !isSupport(actor)}
                obsConnected={obsConnected}
              />
            ),
            audio: (
              <AudioMixer
                path={path}
                defaultMeters={!operator.enabled}
                scene={status?.current_scene?.uuid ?? status?.current_scene?.name ?? null}
                canControl={canControl && !paused}
                obsConnected={obsConnected}
              />
            ),
            activity: (
              <ActivityFeed
                workspaceId={workspace.id}
                switcherId={switcher.id}
                live={live.events}
              />
            ),
          }}
        />
      </SwitcherPage>
    </div>
  );
}

/** A tabbed pane holding BELABOX and Moblin, for layouts short on room. */
function DevicesPane({
  tab,
  onTab,
  belabox,
  moblin,
}: {
  tab: "belabox" | "moblin";
  onTab: ((tab: "belabox" | "moblin") => void) | undefined;
  belabox: ReactNode;
  moblin: ReactNode;
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3.5">
      {onTab && (
        <div className="flex gap-1 self-start rounded-[9px] border bg-muted p-[3px]">
          {(["belabox", "moblin"] as const).map((name) => (
            <button
              key={name}
              type="button"
              aria-pressed={tab === name}
              onClick={() => onTab(name)}
              className={cn(
                "h-6 rounded-md px-2.5 text-xs font-medium text-muted-foreground",
                tab === name && "bg-card text-foreground",
              )}
            >
              {name === "belabox" ? "BELABOX" : "Moblin"}
            </button>
          ))}
        </div>
      )}
      {tab === "belabox" ? belabox : moblin}
    </div>
  );
}

/** Stream, record, fix and marker as a grid, on phones (the desktop header holds the first two). */
function MobileActions({
  path,
  controls,
  status,
  disabled,
}: {
  path: { workspace_id: string; switcher_id: string };
  controls: ObsControls;
  status: SwitcherStatus | null | undefined;
  disabled: boolean;
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
    <section aria-label={t`Actions`} className="flex flex-col gap-2">
      <PaneTitleRow layoutOnly>
        <h2 className="font-semibold">
          <Trans>Actions</Trans>
        </h2>
      </PaneTitleRow>
      <div className="grid grid-cols-2 gap-2">
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
  shortLabel,
  interactive,
  ...rest
}: {
  ok: boolean;
  label: string;
  shortLabel?: string;
  interactive?: boolean;
} & ComponentProps<"button">) {
  const content = (
    <>
      <span className={cn("size-1.5 shrink-0 rounded-full", ok ? "bg-live" : "bg-offline")} />
      {/* Labels can carry a user-chosen computer name. */}
      {shortLabel && <span className="live-narrow">{shortLabel}</span>}
      <span className={cn("max-w-56 truncate", shortLabel && "live-wide")}>{label}</span>
      <span className="sr-only">{ok ? t`connected` : t`not connected`}</span>
    </>
  );
  const className =
    "flex h-9 min-w-0 items-center gap-2 rounded-full border px-3 text-[13px] text-muted-foreground";
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
