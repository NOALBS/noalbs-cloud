import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { type ReactNode, useState } from "react";
import { streamServersQuery, switchersQuery } from "@/api/queries";
import { ConnectDialog } from "@/components/connect-controls";
import { ActivityFeed } from "@/components/live/activity-feed";
import { AudioMixer } from "@/components/live/audio-mixer";
import { BelaboxCard } from "@/components/live/belabox-card";
import { BelaboxSummary, MoblinSummary } from "@/components/live/device-summary";
import { LiveHero } from "@/components/live/hero";
import { columnCount, type TileId, tileNames } from "@/components/live/layout";
import { LiveLayoutEditor } from "@/components/live/layout-editor";
import { LiveBanners } from "@/components/live/live-banners";
import { liveState } from "@/components/live/live-state";
import { Markers, useLiveMarkers } from "@/components/live/markers";
import { MoblinCard } from "@/components/live/moblin-card";
import { ObsPreview } from "@/components/live/obs-preview";
import { ObsStatsBody } from "@/components/live/obs-stats-popover";
import { PhoneActivity, PhoneDevices, PhoneSections, PhoneStatus } from "@/components/live/phone";
import { PhoneActions } from "@/components/live/phone-actions";
import { PhoneScenes } from "@/components/live/phone-scenes";
import {
  AutoSwitchToggle,
  FixButton,
  MarkerButton,
  RecordButton,
  StreamButton,
  useObsControls,
} from "@/components/live/quick-actions";
import { SceneGrid } from "@/components/live/scene-grid";
import { ServerCard } from "@/components/live/server-card";
import { SourceList } from "@/components/live/source-list";
import { LiveTile } from "@/components/live/tile";
import { useLiveLayout } from "@/components/live/use-live-layout";
import { useConnectObsCommand } from "@/hooks/use-connect-obs-command";
import { useMediaQuery } from "@/hooks/use-media-query";
import { useNow } from "@/hooks/use-now";
import { useSwitcher } from "@/hooks/use-switcher";
import { canControlConnect, canManageConnectPrograms } from "@/lib/connect";
import { can, isOwner, isSupport, seesPreviewOffAir } from "@/lib/permissions";
import { setupProgress } from "@/lib/setup";
import { cn } from "@/lib/utils";
import { isStale } from "@/live/reducer";
import { useSwitcherLive } from "@/live/use-switcher-live";

export const Route = createFileRoute("/_app/$workspace/s/$switcher/live")({ component: LivePage });

function LivePage() {
  const { workspace, switcher, actor, path, me } = useSwitcher();
  const live = useSwitcherLive(workspace.id, switcher.id);
  const servers = useQuery(streamServersQuery(workspace.id, switcher.id));
  const summaries = useQuery(switchersQuery(workspace.id));
  const controls = useObsControls(path);
  const obsCommand = useConnectObsCommand(path);
  const now = useNow();
  const [previewVisible, setPreviewVisible] = useState(true);
  const wide = useMediaQuery("(min-width: 1920px)");
  const desktop = useMediaQuery("(min-width: 768px)");
  const cols = columnCount(wide ? 1920 : 0);
  const availability = {
    belabox: switcher.belabox.has_remote_key,
    moblin: switcher.moblin.has_password,
  };
  const layout = useLiveLayout(me.id, switcher.id, cols, availability);
  const summary = summaries.data?.find((summary) => summary.id === switcher.id);
  const status =
    live.connection === "reconnecting"
      ? live.status
      : live.status && !isStale(live.status, now)
        ? live.status
        : live.status === undefined
          ? undefined
          : null;
  const paused = Boolean(summary?.paused || status?.paused || workspace.suspension);
  const markers = useLiveMarkers(workspace.id, switcher.id, Boolean(status?.streaming));
  const canControl = can(actor, "switcher.control");
  const enabled = canControl && !paused;
  const canControlConnectObs = canControlConnect(switcher.connect_obs_control, actor) && !paused;
  const disabled = !enabled || !status?.obs_connected;
  const activeServer = servers.data?.find((server) => server.id === status?.active_server_id);
  const params = { workspace: workspace.url_slug, switcher: switcher.id };
  const progress =
    servers.data && summaries.data
      ? setupProgress({
          connected: Boolean(summary?.status?.obs_connected) || switcher.last_seen_at != null,
          serverCount: servers.data.length,
          scenes: switcher.scenes,
        })
      : null;
  const state = liveState({
    status,
    connection: live.connection,
    paused,
    suspended: Boolean(workspace.suspension),
    setupProgress: progress,
  });
  const feedDown = live.connection === "reconnecting";
  const names = tileNames();
  const belabox = (
    <BelaboxCard
      status={live.belabox}
      history={live.belaboxHistory}
      now={now}
      path={path}
      canControl={enabled}
      canConfigure={enabled && !isSupport(actor)}
      canManage={can(actor, "switcher.manage") && !paused}
      labels={switcher.belabox.interface_labels}
      settingsLink={params}
      paused={paused}
    />
  );
  const moblin = (
    <MoblinCard
      status={live.moblin}
      history={live.moblinHistory}
      now={now}
      path={path}
      canControl={enabled}
      paused={paused}
    />
  );
  const autoSwitch = (
    <AutoSwitchToggle
      controls={controls}
      status={status}
      autoSwitchSetting={switcher.behaviour.bitrate_switcher_enabled}
      onlyWhileStreaming={switcher.behaviour.only_switch_when_streaming}
      canControl={enabled}
      className="rounded-none border-0 border-t border-divider bg-transparent px-0"
    />
  );
  function content(id: TileId, span: number): ReactNode {
    switch (id) {
      case "hero":
        return (
          <LiveHero
            state={state}
            status={status}
            servers={servers.data ?? []}
            activeServer={activeServer}
            live={live}
            now={now}
            low={switcher.triggers.low}
            offline={switcher.triggers.offline}
            scenesSet={Boolean(progress?.scenesSet)}
            narrow={span < 8}
            paused={paused}
          />
        );
      case "actions":
        return (
          <div className="space-y-3">
            <div className="grid grid-cols-2 gap-2">
              <StreamButton
                controls={controls}
                status={status}
                disabled={disabled}
                variant="tile"
              />
              <RecordButton
                controls={controls}
                status={status}
                disabled={disabled}
                variant="tile"
              />
              <FixButton controls={controls} disabled={disabled} variant="tile" />
              <MarkerButton
                controls={controls}
                status={status}
                disabled={disabled}
                variant="tile"
                markerCount={markers.markers?.length}
              />
            </div>
            {autoSwitch}
          </div>
        );
      case "servers":
        return (
          <div className="divide-y divide-divider">
            {servers.isPending && (
              <p className="py-3 text-sm text-muted-foreground">
                <Trans>Loading…</Trans>
              </p>
            )}
            {servers.isError && (
              <p className="py-3 text-sm text-muted-foreground">
                <Trans>Could not load stream servers.</Trans>
              </p>
            )}
            {servers.data?.length === 0 && (
              <p className="py-3 text-sm text-muted-foreground">
                <Trans>No stream servers yet.</Trans>
              </p>
            )}
            {servers.data?.map((server) => (
              <ServerCard
                key={server.id}
                server={server}
                points={live.history[server.id] ?? []}
                now={now}
                inUse={server.id === status?.active_server_id}
              />
            ))}
          </div>
        );
      case "scenes":
        return (
          <SceneGrid
            path={path}
            scenes={switcher.scenes}
            currentUuid={status?.current_scene?.uuid}
            canControl={enabled}
            canManage={can(actor, "switcher.manage") && !paused && !isSupport(actor)}
            obsConnected={Boolean(status?.obs_connected)}
            streaming={Boolean(status?.streaming)}
            feedState={status?.state}
          />
        );
      case "sources":
        return (
          <div
            className={cn(
              "grid items-start gap-4",
              previewVisible && "@min-[480px]:grid-cols-[45%_1fr]",
            )}
          >
            <ObsPreview
              path={path}
              obsConnected={Boolean(status?.obs_connected)}
              streaming={Boolean(status?.streaming)}
              offAirAllowed={seesPreviewOffAir(actor, switcher.preview_off_air)}
              privacyLink={isOwner(actor) ? params : undefined}
              onVisibilityChange={setPreviewVisible}
            />
            <SourceList
              path={path}
              scene={status?.current_scene?.uuid ?? status?.current_scene?.name ?? null}
              canControl={enabled}
              canEditSources={can(actor, "switcher.manage") && !paused && !isSupport(actor)}
              obsConnected={Boolean(status?.obs_connected)}
              columns={!previewVisible}
            />
          </div>
        );
      case "audio":
        return (
          <AudioMixer
            path={path}
            scene={status?.current_scene?.uuid ?? status?.current_scene?.name ?? null}
            canControl={enabled}
            obsConnected={Boolean(status?.obs_connected)}
          />
        );
      case "activity":
        return (
          <ActivityFeed workspaceId={workspace.id} switcherId={switcher.id} live={live.events} />
        );
      case "belabox":
        return belabox;
      case "moblin":
        return moblin;
      case "devices":
        return (
          <div
            className={cn(
              "grid gap-4",
              availability.belabox &&
                availability.moblin &&
                "@min-[400px]:grid-cols-2 @min-[400px]:divide-x divide-divider",
            )}
          >
            {availability.belabox && (
              <div className="@min-[400px]:pr-4">
                <BelaboxSummary
                  status={live.belabox}
                  now={now}
                  labels={switcher.belabox.interface_labels}
                  paused={paused}
                  details={belabox}
                />
              </div>
            )}
            {availability.moblin && (
              <MoblinSummary status={live.moblin} now={now} paused={paused} details={moblin} />
            )}
            {!availability.belabox && !availability.moblin && (
              <p className="text-sm text-muted-foreground">
                <Trans>No devices connected.</Trans>{" "}
                <Link to="/$workspace/s/$switcher/obs" params={params} className="underline">
                  <Trans>Open settings</Trans>
                </Link>
              </p>
            )}
          </div>
        );
      case "obs":
        return status?.obs_connected ? (
          <ObsStatsBody path={path} />
        ) : (
          <p className="text-sm text-muted-foreground">
            <Trans>OBS isn’t connected.</Trans>
          </p>
        );
      case "markers":
        return <Markers {...markers} />;
    }
  }
  return (
    <div
      className={cn(
        "flex w-full flex-col gap-4 px-4 py-4 md:px-6",
        !desktop && "pb-[calc(6rem+env(safe-area-inset-bottom))]",
      )}
    >
      <h1 className="sr-only">
        <Trans>Live</Trans>
      </h1>
      <LiveBanners
        workspace={workspace}
        switcher={switcher}
        paused={paused}
        status={status}
        feedDown={feedDown}
        canControlConnectObs={canControlConnectObs}
        obsCommand={obsCommand}
      />
      {state === "firstRun" && progress ? (
        <LiveSetup name={switcher.name} progress={progress} params={params} />
      ) : !desktop ? (
        <>
          <PhoneStatus
            record={
              <RecordButton
                controls={controls}
                status={status}
                disabled={disabled}
                variant="compact"
              />
            }
            state={state}
            status={status}
            servers={servers.data ?? []}
            activeServer={activeServer}
            live={live}
            now={now}
            low={switcher.triggers.low}
            offline={switcher.triggers.offline}
            scenesSet={Boolean(progress?.scenesSet)}
            narrow
            paused={paused}
          />
          <PhoneSections sections={layout.sections} updateSections={layout.updateSections}>
            {(id) =>
              id === "servers" ? (
                content("servers", 3)
              ) : id === "scenes" ? (
                <PhoneScenes
                  path={path}
                  roles={switcher.scenes}
                  currentUuid={status?.current_scene?.uuid}
                  currentName={status?.current_scene?.name}
                  canControl={enabled}
                  obsConnected={Boolean(status?.obs_connected)}
                  streaming={Boolean(status?.streaming)}
                  feedState={status?.state ?? "idle"}
                />
              ) : id === "devices" ? (
                <PhoneDevices
                  availability={availability}
                  belabox={live.belabox}
                  moblin={live.moblin}
                  now={now}
                  belaboxCard={belabox}
                  moblinCard={moblin}
                  fallback={
                    <>
                      <Trans>No devices connected.</Trans>{" "}
                      <Link to="/$workspace/s/$switcher/obs" params={params} className="underline">
                        <Trans>Open settings</Trans>
                      </Link>
                    </>
                  }
                />
              ) : (
                <PhoneActivity
                  workspaceId={workspace.id}
                  switcherId={switcher.id}
                  live={live.events}
                />
              )
            }
          </PhoneSections>
          <PhoneActions
            controls={controls}
            status={status}
            disabled={disabled}
            canControl={enabled}
            autoSwitchSetting={switcher.behaviour.bitrate_switcher_enabled}
          />
        </>
      ) : (
        <LiveLayoutEditor
          layout={layout}
          cols={cols}
          desktop={desktop}
          availability={availability}
          className={cn(feedDown && "opacity-50")}
        >
          {({ id, span }) => (
            <LiveTile
              key={id}
              title={
                id === "sources" && status?.current_scene
                  ? `${names[id]} · ${status.current_scene.name}`
                  : names[id]
              }
              needsObs={
                !status?.obs_connected &&
                ["actions", "scenes", "sources", "audio", "obs"].includes(id)
              }
              right={
                id === "servers" ? (
                  <Link
                    to="/$workspace/s/$switcher/servers"
                    params={params}
                    className="hover:underline"
                  >
                    <Trans>Manage</Trans>
                  </Link>
                ) : id === "activity" ? (
                  t`This stream`
                ) : id === "actions" && status?.pc ? (
                  <ConnectDialog
                    path={path}
                    deviceName={status.pc.device_name ?? t`Connect computer`}
                    obs={status.pc.obs}
                    streaming={status.streaming}
                    canControlObs={canControlConnectObs}
                    canControlPrograms={
                      canManageConnectPrograms(switcher.connect_programs, actor) && !paused
                    }
                    settingsLink={
                      <Link to="/$workspace/s/$switcher/obs" params={params}>
                        <Trans>Connect settings</Trans>
                      </Link>
                    }
                  >
                    <button
                      type="button"
                      className="hover:underline"
                      aria-label={t`Connect controls`}
                    >
                      Connect
                    </button>
                  </ConnectDialog>
                ) : undefined
              }
            >
              {content(id, span)}
            </LiveTile>
          )}
        </LiveLayoutEditor>
      )}
    </div>
  );
}

function LiveSetup({
  name,
  progress,
  params,
}: {
  name: string;
  progress: ReturnType<typeof setupProgress>;
  params: { workspace: string; switcher: string };
}) {
  const steps = [
    { title: t`Add the browser source to OBS`, done: progress.connected },
    { title: t`Connect your stream server`, done: progress.hasServer },
    { title: t`Pick your LIVE, LOW and BRB scenes`, done: progress.scenesSet },
  ];
  const next = steps.findIndex((step) => !step.done);
  return (
    <section className="mx-auto w-full max-w-2xl rounded-2xl border bg-card p-6 shadow-tile">
      <h2 className="font-display text-2xl font-bold">
        <Trans>Set up {name}</Trans>
      </h2>
      <p className="mt-2 text-sm text-muted-foreground">
        <Trans>Three steps, a couple of minutes. The Live page appears when they’re done.</Trans>
      </p>
      <div
        role="progressbar"
        aria-label={t`Setup progress`}
        aria-valuemin={0}
        aria-valuemax={3}
        aria-valuenow={progress.done}
        className="my-5 h-1 overflow-hidden rounded bg-muted"
      >
        <div
          className="h-full bg-foreground"
          style={{ width: `${(progress.done / progress.total) * 100}%` }}
        />
      </div>
      <ol className="divide-y divide-divider">
        {steps.map((step, index) => (
          <li key={step.title}>
            <Link
              to="/$workspace/s/$switcher/setup"
              params={params}
              className="flex items-center gap-3 py-4 hover:underline"
            >
              <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-muted text-xs">
                {step.done ? "✓" : index + 1}
              </span>
              <span className="flex-1 text-sm font-semibold">{step.title}</span>
              <span className="text-xs text-muted-foreground">
                {step.done ? t`Done` : index === next ? t`Next` : t`After step ${index}`}
              </span>
            </Link>
          </li>
        ))}
      </ol>
    </section>
  );
}
