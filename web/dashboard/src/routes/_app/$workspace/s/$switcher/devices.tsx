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
import { useDeviceSummary } from "@/hooks/use-device-summary";
import { useSwitcher } from "@/hooks/use-switcher";
import { formatKbps } from "@/lib/live-format";
export const Route = createFileRoute("/_app/$workspace/s/$switcher/devices")({
  component: DevicesPage,
});
function DevicesPage() {
  const { canManage } = useSwitcher();
  const devices = useDeviceSummary();
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
              <p className="text-sm font-semibold">{devices.belaboxText}</p>
              {devices.belabox?.streaming && (
                <Badge className="bg-live/10 text-live">{t`Streaming`}</Badge>
              )}
              {devices.belabox && (
                <dl className="flex flex-wrap gap-x-6 gap-y-3 py-2">
                  <div>
                    <dt className="text-xs text-muted-foreground">{t`Bitrate sent`}</dt>
                    <dd className="font-display text-xl font-bold">{t`${formatKbps(devices.belabox.total_kbps)} kbps`}</dd>
                  </div>
                  {devices.belabox.max_br != null && (
                    <div>
                      <dt className="text-xs text-muted-foreground">{t`Max bitrate`}</dt>
                      <dd className="font-display text-xl font-bold">{t`${formatKbps(devices.belabox.max_br)} kbps`}</dd>
                    </div>
                  )}
                </dl>
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
              <p className="text-sm font-semibold">{devices.moblinText}</p>
              {devices.moblin?.streaming && (
                <Badge className="bg-live/10 text-live">{t`Streaming`}</Badge>
              )}
              {devices.moblin && (
                <dl className="flex flex-wrap gap-x-6 gap-y-3 py-2">
                  {(devices.moblin.bitrate_kbps != null || devices.moblin.bitrate != null) && (
                    <div>
                      <dt className="text-xs text-muted-foreground">{t`Bitrate sent`}</dt>
                      <dd className="font-display text-xl font-bold">
                        {devices.moblin.bitrate_kbps != null
                          ? t`${formatKbps(devices.moblin.bitrate_kbps)} kbps`
                          : devices.moblin.bitrate}
                      </dd>
                    </div>
                  )}
                  {devices.moblin.battery_level != null && (
                    <div>
                      <dt className="text-xs text-muted-foreground">{t`Battery`}</dt>
                      <dd className="font-display text-xl font-bold">{t`${devices.moblin.battery_level}%`}</dd>
                    </div>
                  )}
                </dl>
              )}
              <p className="text-sm text-muted-foreground">{t`On Live: bitrate, battery, bonding and remote controls.`}</p>
            </div>
            <MoblinSettings />
          </Section>
        </div>
        <div id="connect" className="min-w-0 scroll-mt-24">
          <ConnectSettings
            summary={
              <p className="border-t border-divider px-5 py-4 text-sm font-semibold">
                {devices.connectText}
              </p>
            }
          />
        </div>
      </div>
    </SwitcherPage>
  );
}
