import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { ExternalLink, Pin, PinOff, X } from "lucide-react";
import {
  meQuery,
  streamServersQuery,
  switcherQuery,
  switchersQuery,
  workspaceBySlugQuery,
} from "@/api/queries";
import type { Me, Workspace } from "@/api/types";
import { ActivityFeed } from "@/components/live/activity-feed";
import { BitrateChart } from "@/components/live/bitrate-chart";
import {
  AutoSwitchToggle,
  FixButton,
  MarkerButton,
  StreamButton,
  useObsControls,
} from "@/components/live/quick-actions";
import { SceneGrid } from "@/components/live/scene-grid";
import { ServerCard } from "@/components/live/server-card";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useNow } from "@/hooks/use-now";
import { errorMessage } from "@/lib/errors";
import { formatKbps, formatUptime } from "@/lib/live-format";
import { organizationRoleLabels } from "@/lib/organizations";
import { actorOf, roleLabels } from "@/lib/permissions";
import { cn } from "@/lib/utils";
import { latest } from "@/live/reducer";
import { useSwitcherLive } from "@/live/use-switcher-live";
import { compactAvailability } from "./compact-state";
import { problemLine, stateChip } from "./format";
import { classifyAttention, type OperatorItem } from "./model";
import type { OperatorPin } from "./pins";

interface Props {
  pin: OperatorPin;
  pinned: boolean;
  canPin: boolean;
  onPin: () => void;
  onClose: () => void;
  column?: boolean;
}

export function CompactLive(props: Props) {
  const workspace = useQuery(workspaceBySlugQuery(props.pin.workspace));
  const me = useQuery(meQuery);
  if (workspace.isError || me.isError)
    return (
      <div className="rounded-xl border p-4">
        <p role="alert">{errorMessage(workspace.error ?? me.error)}</p>
        <Button onClick={props.onClose}>
          <Trans>Close</Trans>
        </Button>
      </div>
    );
  if (!workspace.data || !me.data)
    return (
      <div className="flex flex-col gap-2">
        <Button variant="ghost" onClick={props.onClose}>
          <Trans>Close</Trans>
        </Button>
        <Skeleton className="h-72" />
      </div>
    );
  return (
    <CompactBody
      key={`${props.pin.workspace}/${props.pin.switcher}`}
      {...props}
      workspace={workspace.data}
      me={me.data}
    />
  );
}

function CompactBody({
  pin,
  pinned,
  canPin,
  onPin,
  onClose,
  column,
  workspace,
  me,
}: Props & { workspace: Workspace; me: Me }) {
  const details = useQuery(switcherQuery(workspace.id, pin.switcher));
  const servers = useQuery(streamServersQuery(workspace.id, pin.switcher));
  const summaries = useQuery(switchersQuery(workspace.id));
  const live = useSwitcherLive(workspace.id, pin.switcher);
  const path = { workspace_id: workspace.id, switcher_id: pin.switcher };
  const controls = useObsControls(path);
  const now = useNow();
  const actor = actorOf(workspace, me);
  const paused = Boolean(
    summaries.data?.find((item) => item.id === pin.switcher)?.paused || live.status?.paused,
  );
  const availability = compactAvailability(actor, live.status, live.connection, paused, now);
  const status = availability.status;
  const activeServer =
    servers.data?.find((server) => server.id === status?.active_server_id) ?? servers.data?.[0];
  const sample = activeServer && status?.obs_connected ? latest(live, activeServer.id) : undefined;
  const role = actor.role
    ? roleLabels()[actor.role]
    : actor.organization
      ? organizationRoleLabels()[actor.organization]
      : t`Support`;
  const switcher = details.data;
  const item: OperatorItem = {
    switcher_id: pin.switcher,
    switcher_name: switcher?.name ?? pin.switcher,
    workspace_id: workspace.id,
    workspace_name: workspace.name,
    workspace_slug: workspace.url_slug,
    connected: Boolean(status),
    can_control: availability.control,
    obs_connected: status?.obs_connected ?? false,
    streaming: status?.streaming ?? false,
    recording: status?.recording ?? false,
    paused,
    state: status?.state ?? "idle",
    state_since: status?.state_since,
    stream_started_at: status?.stream_started_at,
    obs_lost: status?.obs_lost,
    stops_at: status?.stops_at,
    ...(status ? classifyAttention(status) : { attention: null, attention_since: null }),
  };
  const chip = stateChip(item);
  const problem = problemLine({ ...item, stops_at: null }, now);
  const remaining = status?.stops_at
    ? formatUptime(new Date(now).toISOString(), Date.parse(status.stops_at))
    : null;
  return (
    <section
      aria-label={t`Live controls for ${item.switcher_name}`}
      className="flex min-w-0 flex-col gap-4 rounded-xl border border-divider bg-background p-3"
    >
      <header className="flex flex-wrap items-center gap-2 border-b border-divider pb-3">
        <h2 className="min-w-0 flex-1 text-sm font-semibold">
          {item.switcher_name}
          <span className="font-normal text-muted-foreground">
            {" "}
            / {workspace.name} · {role}
          </span>
        </h2>
        <Button asChild variant="ghost" size="sm">
          <Link
            to="/$workspace/s/$switcher/live"
            params={{ workspace: workspace.url_slug, switcher: pin.switcher }}
          >
            <ExternalLink className="size-3" />
            <Trans>Open Live</Trans>
          </Link>
        </Button>
        {!column && (
          <Button
            size="icon-sm"
            variant="ghost"
            onClick={onPin}
            disabled={!pinned && !canPin}
            aria-label={pinned ? t`Unpin switcher` : t`Pin switcher`}
          >
            {pinned ? <PinOff className="size-4" /> : <Pin className="size-4" />}
          </Button>
        )}
        <Button
          size="icon-sm"
          variant="ghost"
          onClick={onClose}
          aria-label={column ? t`Unpin switcher` : t`Close live panel`}
        >
          <X className="size-4" />
        </Button>
      </header>
      {details.isError && (
        <p role="alert" className="text-sm text-destructive">
          {errorMessage(details.error)}
        </p>
      )}
      {details.isPending && <Skeleton className="h-24" />}
      <div className="flex flex-col gap-3 rounded-xl border bg-card p-4">
        <div className="flex items-start justify-between gap-2">
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <span className={cn("rounded px-1.5 py-0.5 font-semibold", chip.className)}>
              {live.connection === "reconnecting"
                ? t`RECONNECTING…`
                : live.connection === "connecting"
                  ? t`CONNECTING…`
                  : chip.label}
            </span>
            <span className="text-muted-foreground">
              {status?.streaming && status.stream_started_at
                ? formatUptime(status.stream_started_at, now)
                : "—"}
            </span>
          </div>
          <div className="text-right text-xs">
            <p className="text-[10px] text-muted-foreground">
              <Trans>ON AIR</Trans>
            </p>
            <p>{status?.current_scene?.name ?? t`Unknown scene`}</p>
          </div>
        </div>
        {problem ? (
          <div>
            <p className="font-display text-xl font-bold">{problem}</p>
            {remaining && (
              <p className="mt-1 text-xs text-muted-foreground">
                <Trans>The stream stops in {remaining} if no feed returns.</Trans>
              </p>
            )}
          </div>
        ) : (
          <div className="flex items-baseline gap-3">
            <p className="font-display text-3xl font-bold">
              {status?.streaming && !paused && sample ? formatKbps(sample.bitrate) : "—"}
              <span className="ml-1 text-xs font-normal text-muted-foreground">
                <Trans>kbps</Trans>
              </span>
            </p>
            <span className="text-xs text-muted-foreground">
              {status?.streaming && !paused && sample?.rtt != null
                ? t`${Math.round(sample.rtt)} ms`
                : "—"}
            </span>
          </div>
        )}
        {status?.streaming && (
          <BitrateChart
            points={status?.streaming && activeServer ? (live.history[activeServer.id] ?? []) : []}
            now={now}
            low={switcher?.triggers.low}
            offline={switcher?.triggers.offline}
            tone={chip.className
              .split(" ")
              .filter((name) => name.startsWith("text-"))
              .join(" ")}
            height={80}
          />
        )}
      </div>
      {switcher && (
        <>
          <fieldset
            aria-label={t`Stream controls`}
            className="flex flex-wrap items-center gap-1 [&>button]:gap-1 [&>button]:text-xs [&>button]:px-1.5 [&_svg]:size-3"
          >
            <StreamButton
              controls={controls}
              status={status}
              disabled={!availability.control}
              variant="compact"
            />
            <FixButton controls={controls} disabled={!availability.control} variant="compact" />
            <MarkerButton
              controls={controls}
              status={status}
              disabled={!availability.control}
              variant="compact"
            />
            <AutoSwitchToggle
              controls={controls}
              status={status}
              autoSwitchSetting={switcher.behaviour.bitrate_switcher_enabled}
              onlyWhileStreaming={switcher.behaviour.only_switch_when_streaming}
              canControl={availability.control}
              className="min-h-10 gap-2 rounded-lg border-0 bg-transparent px-1 py-0 [&>span>span:first-child]:text-xs [&>span>span:last-child]:hidden"
            />
          </fieldset>
          <section aria-label={t`Stream servers`} className="flex flex-col gap-2">
            <h3 className="text-sm font-semibold">
              <Trans>Stream servers</Trans>
            </h3>
            {servers.isError && (
              <p role="alert" className="text-sm text-destructive">
                {errorMessage(servers.error)}
              </p>
            )}
            {servers.isPending && <Skeleton className="h-24" />}
            {servers.data?.length === 0 && (
              <p className="text-xs text-muted-foreground">
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
          </section>
          {(live.belabox || live.moblin) && (
            <p className="text-xs text-muted-foreground">
              {live.belabox && t`BELABOX · ${formatKbps(live.belabox.total_kbps)} kbps`}
              {live.belabox && live.moblin && " · "}
              {live.moblin && t`Moblin · ${live.moblin.bitrate ?? "—"}`}
            </p>
          )}
          <SceneGrid
            path={path}
            scenes={switcher.scenes}
            currentUuid={status?.current_scene?.uuid}
            canControl={availability.control}
            canManage={availability.manage}
            obsConnected={Boolean(status?.obs_connected) && live.connection === "open"}
          />
          <div className="max-h-64 overflow-y-auto">
            <ActivityFeed
              workspaceId={workspace.id}
              switcherId={pin.switcher}
              live={live.events.slice(0, 5)}
            />
          </div>
        </>
      )}
    </section>
  );
}
