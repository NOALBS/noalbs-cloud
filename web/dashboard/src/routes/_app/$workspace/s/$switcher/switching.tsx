import { t } from "@lingui/core/macro";
import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { obsScenesQuery, streamServersQuery } from "@/api/queries";
import { IndexedPage } from "@/components/switcher-page";
import { RightNow } from "@/components/switching/right-now";
import { TriggersPage } from "@/components/switching/rules";
import { ScenesPage } from "@/components/switching/scenes";
import { ServersPage } from "@/components/switching/servers";
import { useSwitcher } from "@/hooks/use-switcher";
export const Route = createFileRoute("/_app/$workspace/s/$switcher/switching")({
  validateSearch: (search: Record<string, unknown>): { server?: string } => ({
    server: typeof search.server === "string" ? search.server : undefined,
  }),
  component: SwitchingPage,
});
function SwitchingPage() {
  const { workspace, switcher } = useSwitcher();
  const servers = useQuery(streamServersQuery(workspace.id, switcher.id));
  const scenes = useQuery(obsScenesQuery(workspace.id, switcher.id));
  return (
    <IndexedPage
      title={t`Switching`}
      description={t`Where the feed comes from, which scene that means, and when to switch.`}
      lead={<RightNow />}
      sections={[
        { id: "servers", title: t`Servers`, count: servers.data?.length, content: <ServersPage /> },
        {
          id: "scenes",
          title: t`Scenes`,
          count: scenes.data?.scenes.length,
          content: <ScenesPage />,
        },
        { id: "rules", title: t`Rules`, content: <TriggersPage /> },
      ]}
    />
  );
}
