import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { AlertTriangle, CheckCircle2, RefreshCw, XCircle } from "lucide-react";
import { useState } from "react";
import { adminStatusQuery } from "@/api/queries";
import type { AdminStatus, ChatStatus, ConnectedSwitcher } from "@/api/types";
import { Stat } from "@/components/admin/list-controls";
import { Page, Section } from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useNow } from "@/hooks/use-now";
import {
  CONNECTED_FILTERS,
  type ConnectedFilter,
  matchesConnected,
  statusProblems,
} from "@/lib/admin";
import { errorMessage } from "@/lib/errors";
import { formatUptime } from "@/lib/live-format";
import { currentLocale } from "@/lib/locale";
import { platformLabel } from "@/lib/platforms";
import { formatRelative } from "@/lib/time";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/_app/admin/status")({
  component: StatusPage,
});

function StatusPage() {
  const status = useQuery(adminStatusQuery);
  const now = useNow(1000);
  const data = status.data;

  return (
    <Page
      title="Status"
      description="What’s running right now. Refreshes every 15 seconds."
      className="max-w-6xl"
      actions={
        <div className="flex items-center gap-3">
          {data && (
            <span className="text-sm text-muted-foreground">
              Updated {Math.max(0, Math.round((now - Date.parse(data.generated_at)) / 1000))}s ago
            </span>
          )}
          <Button
            variant="outline"
            size="sm"
            onClick={() => status.refetch()}
            disabled={status.isFetching}
          >
            <RefreshCw className={cn(status.isFetching && "animate-spin")} />
            Refresh
          </Button>
        </div>
      }
    >
      {!data ? (
        status.isError ? (
          <p className="text-sm">{errorMessage(status.error)}</p>
        ) : (
          <Skeleton className="h-64" />
        )
      ) : (
        <>
          <Attention status={data} />
          <div className="grid grid-cols-2 gap-3 md:grid-cols-3 lg:grid-cols-6">
            <Stat label="Switchers" value={data.switchers.total} />
            <Stat label="Connected" value={data.switchers.connected} hint="Browser source up" />
            <Stat label="OBS connected" value={data.switchers.obs_connected} />
            <Stat
              label="Live now"
              value={data.switchers.streaming}
              hint={
                data.switchers.low + data.switchers.offline > 0
                  ? `${data.switchers.low} low · ${data.switchers.offline} offline`
                  : undefined
              }
            />
            <Stat label="Recording" value={data.switchers.recording} />
            <Stat label="Paused" value={data.switchers.paused} hint="Over plan limit" />
          </div>
          <Connected connected={data.connected} now={now} />
          <ChatService chat={data.chat} />
          <Revoked chat={data.chat} />
        </>
      )}
    </Page>
  );
}

function Attention({ status }: { status: AdminStatus }) {
  const problems = statusProblems(status);
  if (problems.length === 0) {
    return (
      <p className="flex items-center gap-2 rounded-xl border border-live/40 bg-live/10 px-3.5 py-2.5 text-sm">
        <CheckCircle2 className="size-4 shrink-0 text-live" />
        All good.
      </p>
    );
  }
  return (
    <ul className="flex flex-col gap-2" aria-label="Needs attention">
      {problems.map((problem) => (
        <li
          key={problem.text}
          className={cn(
            "flex items-start gap-2 rounded-xl border px-3.5 py-2.5 text-sm",
            problem.severity === "error"
              ? "border-offline/40 bg-offline/10"
              : "border-low/40 bg-low/10",
          )}
        >
          {problem.severity === "error" ? (
            <XCircle className="mt-0.5 size-4 shrink-0 text-offline" />
          ) : (
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-low" />
          )}
          {problem.text}
        </li>
      ))}
    </ul>
  );
}

const STATE_CLASS: Record<ConnectedSwitcher["state"], string> = {
  live: "text-live",
  low: "text-low",
  offline: "text-offline",
  idle: "text-muted-foreground",
};

/** The mono tag in front of a connected switcher: its feed while live, else whether OBS is there. */
function connectedTag(switcher: ConnectedSwitcher): { text: string; className: string } {
  if (switcher.streaming) return { text: switcher.state, className: STATE_CLASS[switcher.state] };
  if (switcher.obs_connected) return { text: "OBS", className: "text-foreground" };
  return { text: "No OBS", className: "text-muted-foreground" };
}

function Connected({ connected, now }: { connected: ConnectedSwitcher[]; now: number }) {
  const [filter, setFilter] = useState<ConnectedFilter>("all");
  const shown = connected.filter((switcher) => matchesConnected(switcher, filter));

  return (
    <Section
      title="Connected switchers"
      description="Browser source connected: live first (longest first), then with OBS."
      actions={
        <fieldset className="flex flex-wrap gap-1" aria-label="Show">
          {CONNECTED_FILTERS.map((option) => (
            <Button
              key={option.value}
              size="xs"
              variant={filter === option.value ? "secondary" : "ghost"}
              aria-pressed={filter === option.value}
              onClick={() => setFilter(option.value)}
            >
              {option.label}
              <span className="font-mono tabular text-muted-foreground">
                {connected.filter((switcher) => matchesConnected(switcher, option.value)).length}
              </span>
            </Button>
          ))}
        </fieldset>
      }
    >
      {shown.length === 0 ? (
        <p className="px-5 pb-5 text-sm text-muted-foreground">
          {connected.length === 0 ? "No browser source is connected." : "None right now."}
        </p>
      ) : (
        <ul>
          {shown.map((switcher) => {
            const tag = connectedTag(switcher);
            return (
              <li key={switcher.switcher_id} className="border-t border-divider">
                <Link
                  to="/$workspace/s/$switcher/live"
                  params={{ workspace: switcher.workspace_slug, switcher: switcher.switcher_id }}
                  className="flex min-h-12 flex-wrap items-center gap-x-3 gap-y-1 px-5 py-2.5 hover:bg-surface-2"
                >
                  <span
                    className={cn(
                      "w-16 shrink-0 font-mono text-xs font-bold uppercase",
                      tag.className,
                    )}
                  >
                    {tag.text}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-sm">
                    <span className="font-medium">{switcher.workspace_name}</span>
                    <span className="text-muted-foreground"> · {switcher.switcher_name}</span>
                    {switcher.owner_username && (
                      <span className="text-muted-foreground"> · {switcher.owner_username}</span>
                    )}
                  </span>
                  {switcher.paused && <Badge variant="outline">Paused</Badge>}
                  {switcher.recording && <Badge variant="outline">REC</Badge>}
                  {switcher.scene && (
                    <span className="truncate text-xs text-muted-foreground">{switcher.scene}</span>
                  )}
                  {switcher.streaming && switcher.stream_started_at && (
                    <span className="font-mono text-xs tabular text-muted-foreground">
                      {formatUptime(switcher.stream_started_at, now)}
                    </span>
                  )}
                </Link>
              </li>
            );
          })}
        </ul>
      )}
    </Section>
  );
}

function ChatService({ chat }: { chat: ChatStatus }) {
  const twitch = chat.twitch;
  const kick = chat.kick;
  const youtube = chat.youtube;
  return (
    <Section
      title="Chat service"
      description={`${chat.active_channels} ${chat.active_channels === 1 ? "channel" : "channels"} in use, ${chat.links} ${chat.links === 1 ? "link" : "links"} to switchers. Environment ${chat.environment}.`}
    >
      <div className="flex flex-col gap-5 px-5 pb-5">
        <div className="flex flex-col gap-2">
          <h3 className="text-sm font-semibold">Instances</h3>
          {chat.instances_error ? (
            <p className="text-sm text-muted-foreground">{chat.instances_error}</p>
          ) : chat.instances.length === 0 ? (
            <p className="text-sm text-muted-foreground">None running.</p>
          ) : (
            <ul className="flex flex-wrap gap-2">
              {chat.instances.map((instance) => (
                <li key={instance.id} className="flex flex-col rounded-xl border px-3 py-2 text-sm">
                  <span className="font-mono font-medium">{instance.id}</span>
                  <span className="text-xs text-muted-foreground">
                    Seen {instance.last_seen_seconds}s ago ·{" "}
                    {instance.shards.length === 0
                      ? "no Twitch shards"
                      : `shards ${instance.shards.join(", ")}`}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>

        {twitch && (
          <div className="flex flex-col gap-2">
            <h3 className="text-sm font-semibold">Twitch</h3>
            {twitch.error && <p className="text-sm text-offline">{twitch.error}</p>}
            <p className="text-sm text-muted-foreground">
              Conduit{" "}
              <span className="font-mono text-foreground">
                {twitch.conduit_id ?? "not created yet"}
              </span>
            </p>
            {twitch.shards.length > 0 && (
              <ul className="flex flex-wrap gap-2" aria-label="Conduit shards">
                {twitch.shards.map((shard) => (
                  <li
                    key={shard.id}
                    className={cn(
                      "rounded-lg border px-2.5 py-1.5 text-xs",
                      shard.status === "enabled"
                        ? "border-live/40"
                        : "border-offline/40 bg-offline/10",
                    )}
                  >
                    <span className="font-mono font-semibold">#{shard.id}</span>{" "}
                    {shard.status === "enabled" ? "enabled" : shard.status.replaceAll("_", " ")}
                    <span className="text-muted-foreground">
                      {" "}
                      · {shard.instance ?? "no instance"}
                    </span>
                  </li>
                ))}
              </ul>
            )}
            <CostBar used={twitch.total_cost} limit={twitch.max_total_cost} />
            <p className="text-sm text-muted-foreground">
              Subscriptions on our conduit:{" "}
              {twitch.conduit_subscriptions_by_type.length === 0
                ? "none"
                : twitch.conduit_subscriptions_by_type
                    .map((count) => `${count.count} ${count.name}`)
                    .join(", ")}{" "}
              (recorded {twitch.recorded}). All of the app’s by status:{" "}
              {twitch.subscriptions_by_status.length === 0
                ? "none"
                : twitch.subscriptions_by_status
                    .map((count) => `${count.count} ${count.name.replaceAll("_", " ")}`)
                    .join(", ")}
              .
            </p>
          </div>
        )}

        {kick && (
          <div className="flex flex-col gap-2">
            <h3 className="text-sm font-semibold">Kick</h3>
            {kick.error ? (
              <p className="text-sm text-offline">{kick.error}</p>
            ) : (
              <p className="text-sm text-muted-foreground">
                {kick.subscriptions ?? "?"} event subscriptions (recorded {kick.recorded}).
              </p>
            )}
          </div>
        )}
        {youtube && (
          <div className="flex flex-col gap-2">
            <h3 className="text-sm font-semibold">YouTube</h3>
            {youtube.error && <p className="text-sm text-offline">{youtube.error}</p>}
            <p className="text-sm text-muted-foreground">
              {youtube.watcher
                ? `${youtube.watcher.streaming} streaming ${youtube.watcher.streaming === 1 ? "channel" : "channels"}, ${youtube.watcher.chats_open} live ${youtube.watcher.chats_open === 1 ? "chat" : "chats"} followed by ${youtube.watcher.instance_id}.`
                : "No instance has followed YouTube chats yet."}
            </p>
            {youtube.last_error && (
              <p className="text-sm text-muted-foreground">
                Last error {new Date(youtube.last_error.at).toLocaleString(currentLocale())}
                {youtube.last_error.quota ? " (quota)" : ""}: {youtube.last_error.message}
              </p>
            )}
          </div>
        )}
        {!twitch && !kick && !youtube && (
          <p className="text-sm text-muted-foreground">
            No chat platform is configured on the API.
          </p>
        )}
      </div>
    </Section>
  );
}

/** EventSub cost against Twitch's limit (subscriptions nobody authorised cost 1). */
function CostBar({ used, limit }: { used: number; limit: number }) {
  if (limit <= 0) return null;
  const share = Math.min(1, used / limit);
  return (
    <div className="flex max-w-md flex-col gap-1">
      <div className="flex justify-between text-xs text-muted-foreground">
        <span>EventSub cost</span>
        <span className="font-mono tabular">
          {used.toLocaleString(currentLocale())} / {limit.toLocaleString(currentLocale())}
        </span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-surface-2" aria-hidden="true">
        <div
          className={cn("h-full rounded-full", share >= 0.8 ? "bg-low" : "bg-primary")}
          style={{ width: `${share * 100}%` }}
        />
      </div>
    </div>
  );
}

function Revoked({ chat }: { chat: ChatStatus }) {
  if (chat.revoked.length === 0) return null;
  return (
    <Section
      title="Revoked chat grants"
      description="The platform refused these. The streamer adds the channel again (Chat page), or the bot is connected again."
    >
      <ul>
        {chat.revoked.map((grant) => (
          <li
            key={`${grant.kind}-${grant.platform}-${grant.login}`}
            className="flex min-h-12 flex-wrap items-center gap-x-3 gap-y-1 border-t border-divider px-5 py-2.5"
          >
            <Badge variant="outline">
              {platformLabel(grant.platform)} {grant.kind}
            </Badge>
            <span className="min-w-0 flex-1 truncate text-sm font-medium">{grant.login}</span>
            <span className="text-xs text-muted-foreground">
              {grant.kind === "bot"
                ? grant.workspace_id && grant.workspace_name
                  ? `${grant.workspace_name}’s own bot`
                  : "Shared NOALBS bot"
                : grant.workspace_name
                  ? `${grant.workspace_name} · ${grant.switchers} ${grant.switchers === 1 ? "switcher" : "switchers"}`
                  : "No longer linked"}
              {" · "}
              {formatRelative(grant.revoked_at)}
            </span>
            {grant.workspace_id && (
              <Link
                to="/admin/workspaces/$workspace"
                params={{ workspace: grant.workspace_id }}
                className="text-sm font-medium text-primary hover:underline"
              >
                Workspace
              </Link>
            )}
          </li>
        ))}
      </ul>
    </Section>
  );
}
