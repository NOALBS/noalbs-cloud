import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { MonitorPlay, PauseCircle, PlugZap, Radio, WifiOff } from "lucide-react";
import type { ComponentProps, ReactNode } from "react";
import { streamServersQuery, switchersQuery } from "@/api/queries";
import type { SwitcherStatus } from "@/api/types";
import { ActivityFeed } from "@/components/live/activity-feed";
import { AudioMixer } from "@/components/live/audio-mixer";
import { BelaboxCard } from "@/components/live/belabox-card";
import { BitrateChart } from "@/components/live/bitrate-chart";
import { lookFor } from "@/components/live/feed-state";
import { MoblinCard } from "@/components/live/moblin-card";
import { ObsPreview } from "@/components/live/obs-preview";
import { ObsStatsPopover } from "@/components/live/obs-stats-popover";
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
import { useNow } from "@/hooks/use-now";
import { useSwitcher } from "@/hooks/use-switcher";
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
  const now = useNow();
  const canControl = can(actor, "switcher.control");

  const status =
    live.status && !isStale(live.status, now)
      ? live.status
      : live.status === undefined
        ? undefined
        : null;
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

  return (
    <SwitcherPage
      title={t`Live`}
      description={`${switcher.name} · ${activeServer ? activeServer.name : t`No stream server`} → OBS`}
      actions={
        <div className="hidden flex-wrap items-center gap-2.5 lg:flex">
          <Health ok={Boolean(status?.browser_source)} label={t`Browser source`} />
          <ObsStatsPopover path={path} connected={Boolean(status?.obs_connected)}>
            <Health ok={Boolean(status?.obs_connected)} label="OBS" interactive />
          </ObsStatsPopover>
          {live.connection === "reconnecting" && <Health ok={false} label={t`Live feed`} />}
          <MarkerButton controls={controls} status={status} disabled={disabled} variant="compact" />
          <FixButton controls={controls} disabled={disabled} variant="compact" />
          <RecordButton controls={controls} status={status} disabled={disabled} variant="compact" />
          <StreamButton controls={controls} status={status} disabled={disabled} variant="compact" />
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
                  <strong>Suspended by NOALBS.</strong> {workspace.suspension.reason} This switcher
                  stays connected but doesn't switch or take commands. Questions:{" "}
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
              <strong>Paused.</strong> This workspace has more switchers than its plan includes, so
              this one stays connected but doesn't switch or take commands. Upgrade the plan, or
              delete a switcher you no longer use.
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
        <div className="flex flex-wrap items-center gap-3 rounded-2xl border border-low/40 bg-low/10 p-4">
          <PlugZap className="size-5 text-low" />
          <p className="flex-1 text-sm">
            <Trans>
              <strong>The browser source can’t reach OBS.</strong> Check that WebSocket is on in OBS
              (Tools → WebSocket Server Settings) and that the port and password match Setup.
            </Trans>
          </p>
          <Link
            to="/$workspace/s/$switcher/setup"
            params={params}
            className="text-sm font-semibold text-primary hover:underline"
          >
            <Trans>Open Setup</Trans>
          </Link>
        </div>
      )}

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-12">
        <StatusCard
          status={status}
          now={now}
          sample={sample}
          serverName={activeServer?.name}
          feedDown={feedDown}
          className="lg:col-span-4"
          autoSwitch={
            <AutoSwitchToggle
              controls={controls}
              status={status}
              autoSwitchSetting={switcher.behaviour.bitrate_switcher_enabled}
              onlyWhileStreaming={switcher.behaviour.only_switch_when_streaming}
              canControl={canControl}
              className="mt-auto hidden lg:flex"
            />
          }
        />

        <MobileActions
          controls={controls}
          status={status}
          disabled={disabled}
          canControl={canControl}
          autoSwitchSetting={switcher.behaviour.bitrate_switcher_enabled}
          onlyWhileStreaming={switcher.behaviour.only_switch_when_streaming}
        />

        <section
          aria-label={t`Bitrate`}
          className={cn(
            "hidden flex-col gap-3 rounded-2xl border bg-card p-5 lg:col-span-8 lg:flex",
            feedDown && "opacity-50",
          )}
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

        <section aria-label={t`Stream servers`} className="flex flex-col gap-2.5 lg:col-span-4">
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
          {servers.data?.map((server) => (
            <ServerCard
              key={server.id}
              server={server}
              points={live.history[server.id] ?? []}
              now={now}
              inUse={server.id === status?.active_server_id}
            />
          ))}
          {switcher.belabox.has_remote_key && (
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
          )}
          {switcher.moblin.has_password && (
            <MoblinCard
              status={live.moblin}
              history={live.moblinHistory}
              now={now}
              path={path}
              canControl={canControl && !paused}
            />
          )}
        </section>

        <div className="flex flex-col gap-4 lg:col-span-5">
          <ObsPreview
            path={path}
            obsConnected={Boolean(status?.obs_connected)}
            streaming={Boolean(status?.streaming)}
            offAirAllowed={seesPreviewOffAir(actor, switcher.preview_off_air)}
            privacyLink={isOwner(actor) ? params : undefined}
          />
          <SceneGrid
            path={path}
            scenes={switcher.scenes}
            currentUuid={status?.current_scene?.uuid}
            canControl={canControl && !paused}
            obsConnected={Boolean(status?.obs_connected)}
          />
          <SourceList
            path={path}
            scene={status?.current_scene?.uuid ?? status?.current_scene?.name ?? null}
            canControl={canControl && !paused}
            obsConnected={Boolean(status?.obs_connected)}
          />
          <AudioMixer
            path={path}
            scene={status?.current_scene?.uuid ?? status?.current_scene?.name ?? null}
            canControl={canControl && !paused}
            obsConnected={Boolean(status?.obs_connected)}
          />
        </div>

        <div className="lg:col-span-3">
          <ActivityFeed workspaceId={workspace.id} switcherId={switcher.id} live={live.events} />
        </div>
      </div>
    </SwitcherPage>
  );
}

/** Stream, record, auto switch, fix and marker as a grid, on phones. */
function MobileActions({
  controls,
  status,
  disabled,
  canControl,
  autoSwitchSetting,
  onlyWhileStreaming,
}: {
  controls: ObsControls;
  status: SwitcherStatus | null | undefined;
  disabled: boolean;
  canControl: boolean;
  autoSwitchSetting: boolean;
  onlyWhileStreaming: boolean;
}) {
  return (
    <section aria-label={t`Quick actions`} className="grid grid-cols-2 gap-2.5 lg:hidden">
      <StreamButton controls={controls} status={status} disabled={disabled} variant="tile" />
      <RecordButton controls={controls} status={status} disabled={disabled} variant="tile" />
      <AutoSwitchToggle
        controls={controls}
        status={status}
        autoSwitchSetting={autoSwitchSetting}
        onlyWhileStreaming={onlyWhileStreaming}
        canControl={canControl}
        className="min-h-16 rounded-2xl bg-card"
      />
      <FixButton controls={controls} disabled={disabled} variant="tile" />
      <MarkerButton
        controls={controls}
        status={status}
        disabled={disabled}
        variant="tile"
        className="col-span-2"
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
  feedDown,
  autoSwitch,
  className,
}: {
  status: SwitcherStatus | null | undefined;
  now: number;
  sample: Point | undefined;
  serverName: string | undefined;
  /** The dashboard's own connection dropped: show the last state, greyed out. */
  feedDown: boolean;
  autoSwitch: ReactNode;
  className?: string;
}) {
  const look = feedDown ? lookFor(undefined) : lookFor(status);

  return (
    <section
      aria-label={t`Stream status`}
      className={cn("flex flex-col gap-5 rounded-2xl border bg-card p-5", look.border, className)}
    >
      <div className="flex items-center justify-between gap-3">
        <span
          className={cn(
            "flex h-9 items-center gap-2.5 rounded-full pr-4 pl-3.5 text-sm font-bold tracking-widest",
            look.soft,
            look.text,
          )}
          role="status"
        >
          <span className={cn("size-2.5 rounded-full", look.fill, look.pulse && "animate-pulse")} />
          {feedDown ? t`RECONNECTING…` : status === undefined ? t`CONNECTING…` : look.label}
        </span>
        <span className="font-mono text-sm text-muted-foreground tabular">
          {status?.streaming && status.stream_started_at
            ? formatUptime(status.stream_started_at, now)
            : "--:--:--"}
        </span>
      </div>

      {/* On phones the big numbers live here; desktop has the chart card. */}
      <div
        className={cn("flex items-end justify-between gap-4 lg:hidden", feedDown && "opacity-50")}
      >
        <Stat label={serverName ? t`Bitrate · ${serverName}` : t`Bitrate`}>
          <span className="text-[44px]">{sample ? formatKbps(sample.bitrate) : "—"}</span>
          <span className="text-base font-normal text-muted-foreground">
            {" "}
            <Trans>kbps</Trans>
          </span>
        </Stat>
        <Stat label="RTT">
          <span className="text-2xl">{sample?.rtt != null ? Math.round(sample.rtt) : "—"}</span>
          <span className="text-sm font-normal text-muted-foreground">
            {" "}
            <Trans>ms</Trans>
          </span>
        </Stat>
      </div>

      <div className="flex flex-col gap-1 max-lg:flex-row max-lg:items-center max-lg:justify-between max-lg:border-t max-lg:border-divider max-lg:pt-4">
        <span className="text-xs tracking-wider text-muted-foreground uppercase">
          <Trans>On air</Trans>
        </span>
        <span className="flex items-center gap-2 text-base font-semibold lg:text-3xl lg:font-bold">
          <Radio className={cn("size-4 lg:hidden", look.text)} />
          {status?.current_scene?.name ?? "—"}
        </span>
      </div>

      {autoSwitch}
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
      <span className={cn("size-1.5 rounded-full", ok ? "bg-live" : "bg-offline")} />
      {label}
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
