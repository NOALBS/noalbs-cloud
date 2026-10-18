import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Bell, BellOff, Info, Plus, X } from "lucide-react";
import { type FormEvent, type ReactNode, useState } from "react";
import { toast } from "sonner";
import { ApiRequestError, api, unwrap } from "@/api/client";
import { chatBotsQuery, providersQuery, queryKeys } from "@/api/queries";
import type { Chat, ChatBot, ChatChannel, ChatPlatform, IgnoredChatter } from "@/api/types";
import { ConfirmButton } from "@/components/billing/confirm-button";
import { Section } from "@/components/page";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { useSwitcher } from "@/hooks/use-switcher";
import { type BotSource, channelConnectUrl, speakingBot, workspaceBotConnectUrl } from "@/lib/chat";
import { errorMessage } from "@/lib/errors";
import { can } from "@/lib/permissions";
import { PLATFORMS, platformLabel } from "@/lib/platforms";
import { cn } from "@/lib/utils";

/** The page to come back to after a grant: this one. */
const here = () => window.location.pathname;

/** The platform as a tinted square with its initial, so names line up whatever the platform. */
function PlatformMark({ platform, small }: { platform: ChatPlatform; small?: boolean }) {
  const info = PLATFORMS.find((item) => item.path === platform);
  return (
    <span
      role="img"
      aria-label={info?.label ?? platform}
      className={cn(
        "flex shrink-0 items-center justify-center font-bold",
        small ? "size-7 rounded-md text-xs" : "size-9 rounded-lg text-sm",
        info?.mark,
      )}
    >
      {(info?.label ?? platform).charAt(0).toUpperCase()}
    </span>
  );
}

/** A list row: platform mark, a name with a line under it, and actions on the right. */
function Row({
  platform,
  title,
  detail,
  children,
}: {
  platform: ChatPlatform;
  title: ReactNode;
  detail: ReactNode;
  children?: ReactNode;
}) {
  return (
    <li className="flex min-h-16 items-center gap-3 border-t border-divider px-5 py-3">
      <PlatformMark platform={platform} />
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="truncate font-semibold">{title}</span>
        <span className="text-sm text-muted-foreground">{detail}</span>
      </span>
      {children && <span className="flex shrink-0 items-center gap-1.5">{children}</span>}
    </li>
  );
}

/** The Twitch/Kick/YouTube channels whose chat controls this switcher. */
export function ChannelsSection({ channels }: { channels: ChatChannel[] | undefined }) {
  const { workspace, switcher, canManage } = useSwitcher();
  const providers = useQuery(providersQuery);
  const available = PLATFORMS.filter((platform) => providers.data?.[platform.path]);
  // The switcher-wide setting (Switching page) comes first.
  const notifies = switcher.behaviour.auto_switch_notification;

  return (
    <Section
      title="Channels"
      description="Connect your channel to use chat commands with this switcher. Turn on alerts to have its automatic scene switches posted in chat."
      actions={
        canManage &&
        !!channels?.length &&
        available.length > 0 && (
          <div className="flex flex-wrap gap-2">
            {available.map((platform) => (
              <Button key={platform.path} variant="outline" size="sm" asChild>
                <a
                  title={`Sign in as the ${platform.label} channel’s owner`}
                  href={channelConnectUrl(workspace.id, switcher.id, platform.path, here())}
                >
                  <Plus />
                  Add {platform.label} channel
                </a>
              </Button>
            ))}
          </div>
        )
      }
    >
      {channels === undefined ? (
        <Skeleton className="mx-5 mb-5 h-14" />
      ) : channels.length === 0 ? (
        <div className="flex flex-col gap-3 border-t border-divider px-5 py-4">
          <p className="text-sm text-muted-foreground">
            No channels yet.{" "}
            {providers.data && available.length === 0
              ? "No chat platform is set up on this server."
              : canManage && "Connect the channel you stream on so its chat can run commands."}
          </p>
          {canManage && available.length > 0 && (
            <>
              <div className="flex flex-wrap gap-2">
                {available.map((platform) => (
                  <Button key={platform.path} className={platform.button} asChild>
                    <a href={channelConnectUrl(workspace.id, switcher.id, platform.path, here())}>
                      Connect {platform.label} channel
                    </a>
                  </Button>
                ))}
              </div>
              <p className="flex items-center gap-2 text-xs text-muted-foreground">
                <Info className="size-3.5 shrink-0" aria-hidden="true" />
                Sign in with the channel’s own account: that lets the NOALBS bot into its chat.
              </p>
              {available.some((platform) => platform.path === "youtube") && (
                <p className="flex items-center gap-2 text-xs text-muted-foreground">
                  <Info className="size-3.5 shrink-0" aria-hidden="true" />
                  On YouTube, commands work while you’re live: the bot finds your live chat once OBS
                  starts streaming.
                </p>
              )}
            </>
          )}
        </div>
      ) : (
        <>
          <ul>
            {channels.map((channel) => (
              <ChannelRow
                key={channel.id}
                channel={channel}
                canManage={canManage}
                notifies={notifies}
              />
            ))}
          </ul>
          {!notifies && (
            <p className="flex items-center gap-2 border-t border-divider px-5 py-3 text-sm text-muted-foreground">
              <BellOff className="size-4 shrink-0" aria-hidden="true" />
              <span>
                Switch alerts are off for this switcher.{" "}
                <Link
                  to="/$workspace/s/$switcher/triggers"
                  params={{ workspace: workspace.url_slug, switcher: switcher.id }}
                  className="font-medium text-primary hover:underline"
                >
                  Turn on
                </Link>
              </span>
            </p>
          )}
        </>
      )}
    </Section>
  );
}

function ChannelRow({
  channel,
  canManage,
  notifies,
}: {
  channel: ChatChannel;
  canManage: boolean;
  /** The switcher posts automatic switches at all. */
  notifies: boolean;
}) {
  const { workspace, switcher, path } = useSwitcher();
  const queryClient = useQueryClient();
  const chatKey = queryKeys.chat(path.workspace_id, path.switcher_id);
  const remove = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE(
          "/workspaces/{workspace_id}/switchers/{switcher_id}/chat/channels/{channel_id}",
          { params: { path: { ...path, channel_id: channel.id } } },
        ),
      ),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: chatKey });
      toast.success(`${channel.display_name} removed`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  const notify = useMutation({
    mutationFn: (enabled: boolean) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/chat/channels/{channel_id}", {
          params: { path: { ...path, channel_id: channel.id } },
          body: { auto_switch_notification: enabled },
        }),
      ),
    onMutate: async (enabled) => {
      await queryClient.cancelQueries({ queryKey: chatKey });
      const previous = queryClient.getQueryData<Chat>(chatKey);
      queryClient.setQueryData<Chat>(
        chatKey,
        (data) =>
          data && {
            ...data,
            channels: data.channels.map((item) =>
              item.id === channel.id ? { ...item, auto_switch_notification: enabled } : item,
            ),
          },
      );
      return { previous };
    },
    onError: (error, _, context) => {
      if (context?.previous) queryClient.setQueryData(chatKey, context.previous);
      toast.error(errorMessage(error));
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: chatKey }),
  });

  const revoked = channel.status === "revoked";
  const on = channel.auto_switch_notification;
  // Who answers is in the Bot card; say what's special about this channel.
  const detail = [
    platformLabel(channel.platform),
    revoked
      ? "the bot’s access was taken away"
      : !channel.bot
        ? "no bot connected, so nothing answers"
        : channel.platform === "youtube"
          ? "answers while you’re live"
          : null,
    channel.other_links > 0 &&
      `also on ${channel.other_links} other ${channel.other_links === 1 ? "switcher" : "switchers"}`,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <Row platform={channel.platform} title={channel.display_name} detail={detail}>
      {revoked && canManage && (
        <Button variant="outline" size="sm" asChild>
          <a href={channelConnectUrl(workspace.id, switcher.id, channel.platform, here())}>
            Reconnect
          </a>
        </Button>
      )}
      {!revoked && notifies && (
        <Button
          variant="ghost"
          size="sm"
          aria-pressed={on}
          aria-label={`Switch alerts in ${channel.display_name}`}
          title={
            on
              ? "Automatic scene switches are posted here"
              : "Automatic scene switches aren’t posted here"
          }
          disabled={!canManage || notify.isPending}
          onClick={() => notify.mutate(!on)}
          className={cn(
            "gap-1.5 disabled:opacity-100",
            on
              ? "bg-primary-soft text-primary-soft-foreground hover:bg-primary-soft"
              : "text-muted-foreground",
          )}
        >
          {on ? <Bell /> : <BellOff />}
          <span className="sm:hidden">{on ? "On" : "Off"}</span>
          <span className="hidden sm:inline">{on ? "Alerts on" : "Alerts off"}</span>
        </Button>
      )}
      {canManage && (
        <ConfirmButton
          variant="ghost"
          className="size-8 p-0 text-muted-foreground"
          ariaLabel={`Remove ${channel.display_name}`}
          title={`Remove ${channel.display_name}?`}
          description={
            channel.other_links > 0
              ? "Its chat stops controlling this switcher. The other switchers using it keep it."
              : "Its chat stops controlling this switcher, and the bot leaves the channel."
          }
          confirmLabel="Remove"
          disabled={remove.isPending}
          onConfirm={() => remove.mutate()}
        >
          <X />
        </ConfirmButton>
      )}
    </Row>
  );
}

/** What to say under a platform's bot. */
function botDetail(
  own: ChatBot | undefined,
  speaking: { source: BotSource } | null,
  customAllowed: boolean,
  managedBy: string | null,
): string {
  const fallback = speaking?.source === "organization" ? `${managedBy}’s bot` : "the shared bot";
  if (own && own.status === "revoked" && customAllowed) {
    return `Your bot ${own.display_name} lost its access; ${fallback} answers until you reconnect it`;
  }
  if (own && !customAllowed) {
    return managedBy
      ? `Your bot ${own.display_name} is off: ${managedBy} doesn’t allow your own bot`
      : `Your bot ${own.display_name} only answers on plans with your own bot`;
  }
  if (!speaking) return "Staff haven’t connected the shared bot yet";
  if (speaking.source === "own") return "Your own bot";
  return speaking.source === "organization" ? `${managedBy}’s bot` : "Shared NOALBS bot";
}

/** Who answers in chat for this workspace, and connecting its own bot. */
export function BotSection() {
  const { workspace, actor } = useSwitcher();
  const bots = useQuery(chatBotsQuery(workspace.id));
  const providers = useQuery(providersQuery);
  const canConnect = can(actor, "account.manage");
  const platforms = PLATFORMS.filter((platform) => providers.data?.[platform.path]);
  const data = bots.data;

  return (
    <Section title="Bot" description="Who replies in chat, for every switcher in this workspace.">
      {!data ? (
        <Skeleton className="mx-5 mb-5 h-14" />
      ) : (
        <ul>
          {platforms.map((platform) => {
            const speaking = speakingBot(data, platform.path);
            const own = data.own.find((bot) => bot.platform === platform.path);
            return (
              <Row
                key={platform.path}
                platform={platform.path}
                title={speaking ? speaking.bot.display_name : `No ${platform.label} bot yet`}
                detail={botDetail(
                  own,
                  speaking,
                  data.custom_allowed,
                  workspace.managed_by?.name ?? null,
                )}
              >
                {data.custom_allowed && canConnect && (
                  <>
                    <Button variant="outline" size="sm" asChild>
                      <a href={workspaceBotConnectUrl(workspace.id, platform.path, here())}>
                        {own ? "Reconnect" : "Use your own bot"}
                      </a>
                    </Button>
                    {own && (
                      <RemoveOwnBot
                        platform={platform.path}
                        name={own.display_name}
                        fallback={
                          workspace.managed_by
                            ? `${workspace.managed_by.name}’s`
                            : "The shared NOALBS"
                        }
                      />
                    )}
                  </>
                )}
              </Row>
            );
          })}
        </ul>
      )}
      {data && !data.custom_allowed && workspace.managed_by && (
        <p className="border-t border-divider px-5 py-3 text-sm text-muted-foreground">
          {workspace.managed_by.name} chooses which bot answers here.
        </p>
      )}
      {data && !data.custom_allowed && !workspace.managed_by && (
        <p className="border-t border-divider px-5 py-3 text-sm text-muted-foreground">
          Want replies from your own bot account?{" "}
          <Link
            to="/$workspace/settings/plan"
            params={{ workspace: workspace.url_slug }}
            className="font-medium text-primary hover:underline"
          >
            See plans
          </Link>
        </p>
      )}
      {data?.custom_allowed && canConnect && (
        <p className="flex items-center gap-2 border-t border-divider px-5 py-3 text-xs text-muted-foreground">
          <Info className="size-3.5 shrink-0" aria-hidden="true" />
          Using your own bot? Sign in as the bot account when asked, not as yourself.
        </p>
      )}
    </Section>
  );
}

function RemoveOwnBot({
  platform,
  name,
  fallback,
}: {
  platform: ChatPlatform;
  name: string;
  /** Whose bot answers instead, e.g. "The shared NOALBS". */
  fallback: string;
}) {
  const { workspace } = useSwitcher();
  const queryClient = useQueryClient();
  const remove = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE("/workspaces/{workspace_id}/chat-bot/{platform}", {
          params: { path: { workspace_id: workspace.id, platform } },
        }),
      ),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["workspaces", workspace.id] });
      toast.success(`${fallback} bot answers again`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  return (
    <ConfirmButton
      variant="ghost"
      title={`Stop using ${name}?`}
      description={`${fallback} bot answers in your channels instead.`}
      confirmLabel="Stop using it"
      disabled={remove.isPending}
      onConfirm={() => remove.mutate()}
    >
      Stop using it
    </ConfirmButton>
  );
}

/** Chatters whose commands this switcher ignores, e.g. other bots. */
export function IgnoredSection({ ignored }: { ignored: IgnoredChatter[] | undefined }) {
  const { canManage, path } = useSwitcher();
  const providers = useQuery(providersQuery);
  const queryClient = useQueryClient();
  const available = PLATFORMS.filter((platform) => providers.data?.[platform.path]);
  const [platform, setPlatform] = useState<ChatPlatform>("twitch");
  const [username, setUsername] = useState("");
  const chosen = available.some((item) => item.path === platform)
    ? platform
    : (available[0]?.path ?? "twitch");

  const refresh = () =>
    queryClient.invalidateQueries({
      queryKey: queryKeys.chat(path.workspace_id, path.switcher_id),
    });
  const add = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/chat/ignored", {
          params: { path },
          body: { platform: chosen, username },
        }),
      ),
    onSuccess: async (chatter) => {
      await refresh();
      setUsername("");
      toast.success(`Ignoring ${chatter.username}`);
    },
    onError: (error) =>
      toast.error(
        error instanceof ApiRequestError && error.code === "user_not_found"
          ? `No ${PLATFORMS.find((item) => item.path === chosen)?.label} user is called ${username.trim()}.`
          : errorMessage(error),
      ),
  });
  const remove = useMutation({
    mutationFn: (chatter: IgnoredChatter) =>
      unwrap(
        api.DELETE(
          "/workspaces/{workspace_id}/switchers/{switcher_id}/chat/ignored/{platform}/{user_id}",
          { params: { path: { ...path, platform: chatter.platform, user_id: chatter.user_id } } },
        ),
      ),
    onSuccess: refresh,
    onError: (error) => toast.error(errorMessage(error)),
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (username.trim()) add.mutate();
  };

  return (
    <Section
      title="Ignored chatters"
      description="Commands from these accounts are ignored, e.g. other bots. Bots connected to NOALBS are always ignored."
    >
      {ignored === undefined ? (
        <Skeleton className="mx-5 mb-5 h-10" />
      ) : (
        <ul>
          {ignored.map((chatter) => (
            <li
              key={`${chatter.platform}-${chatter.user_id}`}
              className="flex min-h-12 items-center gap-3 border-t border-divider px-5 py-2"
            >
              <PlatformMark platform={chatter.platform} small />
              <span className="min-w-0 flex-1 truncate text-sm font-medium">
                {chatter.username}
              </span>
              {canManage && (
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={`Stop ignoring ${chatter.username}`}
                  disabled={remove.isPending}
                  onClick={() => remove.mutate(chatter)}
                >
                  <X />
                </Button>
              )}
            </li>
          ))}
          {ignored.length === 0 && (
            <li className="border-t border-divider px-5 py-3 text-sm text-muted-foreground">
              Nobody is ignored.
            </li>
          )}
        </ul>
      )}
      {canManage && available.length > 0 && (
        <form
          onSubmit={submit}
          className="flex flex-wrap items-center gap-2 border-t border-divider px-5 py-3"
        >
          <Select value={chosen} onValueChange={(value) => setPlatform(value as ChatPlatform)}>
            <SelectTrigger className="w-28" aria-label="Platform">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {available.map((item) => (
                <SelectItem key={item.path} value={item.path}>
                  {item.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Input
            value={username}
            onChange={(event) => setUsername(event.target.value)}
            placeholder={chosen === "youtube" ? "@handle" : "Username"}
            aria-label="Username"
            className="w-48 flex-1 sm:flex-none"
            maxLength={chosen === "youtube" ? 31 : 50}
          />
          <Button type="submit" variant="outline" disabled={!username.trim() || add.isPending}>
            Ignore
          </Button>
        </form>
      )}
    </Section>
  );
}
