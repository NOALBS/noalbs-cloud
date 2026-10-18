import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Bot } from "lucide-react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { organizationChatBotsQuery, providersQuery, queryKeys } from "@/api/queries";
import type { ChatBot, ChatPlatform, Organization } from "@/api/types";
import { ConfirmButton } from "@/components/billing/confirm-button";
import { Section } from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { organizationBotConnectUrl } from "@/lib/chat";
import { errorMessage } from "@/lib/errors";
import { PLATFORMS } from "@/lib/platforms";
import { formatRelative } from "@/lib/time";

/**
 * The bot accounts that answer in all the organization's clients' channels, per
 * platform, instead of the shared NOALBS bot. Admins connect them (signing in as
 * the bot account) and disconnect them.
 */
export function OrganizationBotsSection({
  organization,
  isAdmin,
}: {
  organization: Organization;
  isAdmin: boolean;
}) {
  const bots = useQuery(organizationChatBotsQuery(organization.id));
  const providers = useQuery(providersQuery);

  return (
    <Section
      title="Your bot"
      description={
        isAdmin
          ? "Answers in the chat of all your clients, in place of the shared NOALBS bot. Sign in as the bot account (not your own) when the platform asks."
          : "Answers in the chat of all your clients, in place of the shared NOALBS bot."
      }
    >
      {!bots.data || !providers.data ? (
        <Skeleton className="mx-5 mb-5 h-14" />
      ) : (
        <ul>
          {PLATFORMS.map((platform) => (
            <BotRow
              key={platform.path}
              organization={organization}
              platform={platform.path}
              label={platform.label}
              configured={providers.data[platform.path]}
              bot={bots.data.find((bot) => bot.platform === platform.path)}
              isAdmin={isAdmin}
            />
          ))}
        </ul>
      )}
    </Section>
  );
}

function BotRow({
  organization,
  platform,
  label,
  configured,
  bot,
  isAdmin,
}: {
  organization: Organization;
  platform: ChatPlatform;
  label: string;
  configured: boolean;
  bot: ChatBot | undefined;
  isAdmin: boolean;
}) {
  const queryClient = useQueryClient();
  const remove = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE("/organizations/{organization_id}/chat-bots/{platform}", {
          params: { path: { organization_id: organization.id, platform } },
        }),
      ),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: queryKeys.organizationChatBots(organization.id),
      });
      toast.success(`${label} bot disconnected`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <li className="flex min-h-16 flex-wrap items-center gap-3 border-t border-divider px-5 py-3">
      <Bot className="size-5 shrink-0 text-muted-foreground" />
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="font-semibold">
          {label}
          {bot && `: ${bot.display_name}`}
        </span>
        <span className="text-sm text-muted-foreground">
          {!configured
            ? `${label} sign-in isn’t configured on this server.`
            : bot
              ? `Connected ${formatRelative(bot.connected_at)}`
              : `Not connected: your ${label} clients get the shared NOALBS bot.`}
        </span>
      </span>
      {bot?.status === "revoked" && (
        <Badge className="bg-low/15 text-low">Needs reconnecting</Badge>
      )}
      {isAdmin && configured && (
        <Button variant={bot ? "outline" : "default"} size="sm" asChild>
          <a href={organizationBotConnectUrl(organization.id, platform, window.location.pathname)}>
            {bot ? "Reconnect" : "Connect"}
          </a>
        </Button>
      )}
      {isAdmin && bot && (
        <ConfirmButton
          variant="ghost"
          title={`Disconnect the ${label} bot?`}
          description={`Your ${label} clients get the shared NOALBS bot again.`}
          confirmLabel="Disconnect"
          disabled={remove.isPending}
          onConfirm={() => remove.mutate()}
        >
          Disconnect
        </ConfirmButton>
      )}
    </li>
  );
}

/** Whether customers may connect a chat bot of their own, which then answers instead of yours. */
export function CustomerBotSetting({
  organization,
  isAdmin,
}: {
  organization: Organization;
  isAdmin: boolean;
}) {
  const queryClient = useQueryClient();
  const allowed = organization.settings.custom_chat_bot;
  const update = useMutation({
    mutationFn: (custom_chat_bot: boolean) =>
      unwrap(
        api.PUT("/organizations/{organization_id}/settings", {
          params: { path: { organization_id: organization.id } },
          body: { custom_chat_bot },
        }),
      ),
    onSuccess: async (_, custom_chat_bot) => {
      await queryClient.invalidateQueries({ queryKey: queryKeys.organizations });
      toast.success(custom_chat_bot ? "Customers can use their own bot" : "Customers use your bot");
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title="Customers’ own bots"
      description="Lets a customer connect a bot account of their own on a client’s Chat page. It answers in their channels instead of yours. Turning it off keeps what they connected but stops it answering."
    >
      <div className="flex items-center justify-between gap-4 px-5 pb-5">
        <Label htmlFor="customer-bot" className="flex flex-col items-start gap-0.5">
          <span>Customers can connect their own bot</span>
          <span className="text-xs font-normal text-muted-foreground">
            {allowed ? "On" : "Off: your bot, or the shared NOALBS bot, answers for every client."}
          </span>
        </Label>
        <Switch
          id="customer-bot"
          checked={allowed}
          disabled={!isAdmin || update.isPending}
          onCheckedChange={(next) => update.mutate(next)}
        />
      </div>
    </Section>
  );
}
