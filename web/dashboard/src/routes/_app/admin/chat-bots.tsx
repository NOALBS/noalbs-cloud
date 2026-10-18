import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { Bot } from "lucide-react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { adminChatBotsQuery, providersQuery, queryKeys } from "@/api/queries";
import type { ChatBot, ChatPlatform } from "@/api/types";
import { ConfirmButton } from "@/components/billing/confirm-button";
import { Page, Section } from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useGrantOutcome, validateGrantSearch } from "@/hooks/use-grant-outcome";
import { sharedBotConnectUrl } from "@/lib/chat";
import { errorMessage } from "@/lib/errors";
import { PLATFORMS } from "@/lib/platforms";
import { formatRelative } from "@/lib/time";

export const Route = createFileRoute("/_app/admin/chat-bots")({
  validateSearch: validateGrantSearch,
  component: ChatBotsPage,
});

function ChatBotsPage() {
  useGrantOutcome(Route.useSearch(), (platform) => `Shared ${platform} bot connected`);
  const bots = useQuery(adminChatBotsQuery);
  const providers = useQuery(providersQuery);

  return (
    <Page
      title="Chat bots"
      description="The shared NOALBS bot accounts that answer in every linked channel, unless a workspace uses its own bot."
      className="max-w-4xl"
    >
      <Section
        title="Shared bots"
        description="Connect by signing in as the bot account (not your own) when the platform asks."
      >
        {!bots.data || !providers.data ? (
          <Skeleton className="mx-5 mb-5 h-14" />
        ) : (
          <ul>
            {PLATFORMS.map((platform) => (
              <SharedBotRow
                key={platform.path}
                platform={platform.path}
                label={platform.label}
                configured={providers.data[platform.path]}
                bot={bots.data.find((bot) => bot.platform === platform.path)}
              />
            ))}
          </ul>
        )}
      </Section>
    </Page>
  );
}

function SharedBotRow({
  platform,
  label,
  configured,
  bot,
}: {
  platform: ChatPlatform;
  label: string;
  configured: boolean;
  bot: ChatBot | undefined;
}) {
  const queryClient = useQueryClient();
  const remove = useMutation({
    mutationFn: () =>
      unwrap(api.DELETE("/admin/chat-bots/{platform}", { params: { path: { platform } } })),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: queryKeys.adminChatBots });
      toast.success(`Shared ${label} bot disconnected`);
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
              : `Not connected: ${label} channels without their own bot get no replies.`}
        </span>
      </span>
      {bot?.status === "revoked" && (
        <Badge className="bg-low/15 text-low">Needs reconnecting</Badge>
      )}
      {configured && (
        <Button variant={bot ? "outline" : "default"} size="sm" asChild>
          <a href={sharedBotConnectUrl(platform, window.location.pathname)}>
            {bot ? "Reconnect" : "Connect"}
          </a>
        </Button>
      )}
      {bot && (
        <ConfirmButton
          variant="ghost"
          title={`Disconnect the shared ${label} bot?`}
          description={`${label} channels without their own bot stop getting replies until one is connected again.`}
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
