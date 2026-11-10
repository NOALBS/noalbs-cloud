import { t } from "@lingui/core/macro";
import { createFileRoute } from "@tanstack/react-router";
import { Server, Smartphone } from "lucide-react";
import { BelaboxSettings } from "@/components/belabox-settings";
import { ConnectSettings } from "@/components/connect-settings";
import { MoblinSettings } from "@/components/moblin-settings";
import { Section } from "@/components/page";
import { ReadOnlyNotice } from "@/components/settings";
import { SwitcherPage } from "@/components/switcher-page";
import { Badge } from "@/components/ui/badge";
import { useSwitcher } from "@/hooks/use-switcher";
import { formatKbps } from "@/lib/live-format";
import { useSwitcherLive } from "@/live/use-switcher-live";
export const Route = createFileRoute("/_app/$workspace/s/$switcher/devices")({
  component: DevicesPage,
});
function DevicesPage() {
  const { workspace, switcher, canManage } = useSwitcher();
  const live = useSwitcherLive(workspace.id, switcher.id);
  return (
    <SwitcherPage
      title={t`Devices`}
      description={t`The encoder and the PC this switcher can control. Each one adds a panel to the Live page.`}
      className="max-w-[1500px]"
    >
      {!canManage && <ReadOnlyNotice />}
      <div className="grid items-start gap-4 lg:grid-cols-3">
        <div id="belabox" className="min-w-0 scroll-mt-24">
          <Section
            title="BELABOX"
            description={t`Via BELABOX Cloud remote.`}
            actions={<Server className="size-5 text-muted-foreground" />}
          >
            <div className="space-y-2 border-t border-divider px-5 py-4">
              {live.belabox?.streaming && (
                <Badge className="bg-live/10 text-live">{t`Streaming`}</Badge>
              )}
              {live.belabox && (
                <p className="font-display font-semibold">{formatKbps(live.belabox.total_kbps)}</p>
              )}
              <p className="text-sm text-muted-foreground">{t`On Live: bitrate sent, max bitrate and modem controls.`}</p>
            </div>
            <BelaboxSettings />
          </Section>
        </div>
        <div id="moblin" className="min-w-0 scroll-mt-24">
          <Section
            title="Moblin"
            description={t`iPhone remote control.`}
            actions={<Smartphone className="size-5 text-muted-foreground" />}
          >
            <div className="space-y-2 border-t border-divider px-5 py-4">
              {live.moblin?.streaming && (
                <Badge className="bg-live/10 text-live">{t`Streaming`}</Badge>
              )}
              {live.moblin?.bitrate_kbps != null && (
                <p className="font-display font-semibold">{formatKbps(live.moblin.bitrate_kbps)}</p>
              )}
              {live.moblin?.battery_level != null && (
                <p className="text-sm">{t`Battery: ${live.moblin.battery_level}%`}</p>
              )}
              <p className="text-sm text-muted-foreground">{t`On Live: bitrate, battery, bonding and remote controls.`}</p>
            </div>
            <MoblinSettings />
          </Section>
        </div>
        <div id="connect" className="min-w-0 scroll-mt-24">
          <ConnectSettings />
        </div>
      </div>
    </SwitcherPage>
  );
}
