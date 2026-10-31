import { t } from "@lingui/core/macro";
import { createFileRoute } from "@tanstack/react-router";
import { CustomerBotSetting, OrganizationBotsSection } from "@/components/orgs/chat-bot-section";
import { Page } from "@/components/page";
import { useGrantOutcome, validateGrantSearch } from "@/hooks/use-grant-outcome";
import { useOrganization } from "@/hooks/use-organization";

export const Route = createFileRoute("/_app/orgs/$organization/chat")({
  validateSearch: validateGrantSearch,
  component: ChatBotPage,
});

function ChatBotPage() {
  const { organization, isAdmin } = useOrganization();
  useGrantOutcome(Route.useSearch(), (platform) => t`${platform} bot connected`);

  return (
    <Page title={t`Chat bot`} description={t`Who answers in the chat of your clients’ channels.`}>
      <OrganizationBotsSection organization={organization} isAdmin={isAdmin} />
      <CustomerBotSetting organization={organization} isAdmin={isAdmin} />
    </Page>
  );
}
