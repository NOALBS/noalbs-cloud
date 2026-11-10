import { t } from "@lingui/core/macro";
import { createFileRoute } from "@tanstack/react-router";
import { IndexedPage } from "@/components/switcher-page";
import { ScenesPage } from "@/components/switcher-scenes-content";
import { ServersPage } from "@/components/switcher-servers-content";
import { TriggersPage } from "@/components/switcher-triggers-content";
export const Route = createFileRoute("/_app/$workspace/s/$switcher/switching")({
  component: SwitchingPage,
});
function SwitchingPage() {
  return (
    <IndexedPage
      title={t`Switching`}
      description={t`Where the feed comes from, which scene that means, and when to switch.`}
      sections={[
        { id: "servers", title: t`Servers`, content: <ServersPage /> },
        { id: "scenes", title: t`Scenes`, content: <ScenesPage /> },
        { id: "rules", title: t`Rules`, content: <TriggersPage /> },
      ]}
    />
  );
}
