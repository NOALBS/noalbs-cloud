import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import type { ReactNode } from "react";
import type { BelaboxStatus, MoblinStatus } from "@/api/types";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import { belaboxView, interfaceTitle } from "@/lib/belabox";
import { formatKbps } from "@/lib/live-format";
import { bondingKeys, moblinView, thermalLabel } from "@/lib/moblin";

function DeviceColumn({
  name,
  state,
  paused,
  details,
  children,
}: {
  name: string;
  state: string;
  paused: boolean;
  details: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="min-w-0 space-y-3">
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-sm font-semibold">{name}</h3>
        <span className="text-xs text-muted-foreground">{state}</span>
      </div>
      {children}
      {paused && (
        <p className="text-xs text-muted-foreground">
          <Trans>Controls are off while the switcher is paused.</Trans>
        </p>
      )}
      <Sheet>
        <SheetTrigger asChild>
          <Button variant="outline" size="sm">
            <Trans>Details</Trans>
            <span className="sr-only"> · {name}</span>
          </Button>
        </SheetTrigger>
        <SheetContent side="right" className="w-full overflow-y-auto sm:max-w-2xl">
          <SheetHeader>
            <SheetTitle>{name}</SheetTitle>
            <SheetDescription>
              <Trans>Device status and controls</Trans>
            </SheetDescription>
          </SheetHeader>
          <div className="px-4 pb-6">{details}</div>
        </SheetContent>
      </Sheet>
    </div>
  );
}

export function BelaboxSummary({
  status,
  now,
  labels,
  paused,
  details,
}: {
  status: BelaboxStatus | null | undefined;
  now: number;
  labels: Record<string, string>;
  paused: boolean;
  details: ReactNode;
}) {
  const view = belaboxView(status, now);
  const online = view.kind === "online" ? view.status : null;
  const state = online
    ? online.streaming
      ? t`Streaming`
      : t`Not streaming`
    : view.kind === "loading" || view.kind === "connecting"
      ? t`Connecting…`
      : view.kind === "offline"
        ? t`Offline`
        : t`Not connected`;
  return (
    <DeviceColumn name="BELABOX" state={state} paused={paused} details={details}>
      {online ? (
        <>
          <p className="text-xs text-muted-foreground">
            <span className="font-display text-3xl font-bold text-foreground tabular-nums">
              {formatKbps(online.total_kbps)}
            </span>{" "}
            <Trans>kbps sent</Trans>
          </p>
          <dl className="divide-y divide-divider">
            {online.interfaces.map((netif) => (
              <div
                key={netif.name}
                className="flex items-center justify-between gap-3 py-1.5 text-xs"
              >
                <dt className="truncate text-muted-foreground">{interfaceTitle(netif, labels)}</dt>
                <dd className="font-display tabular-nums">
                  {netif.enabled ? formatKbps(netif.kbps) : t`Off`}
                </dd>
              </div>
            ))}
          </dl>
          <p className="text-xs text-muted-foreground">
            {online.max_br != null && t`Max ${formatKbps(online.max_br)} kbps`}
            {online.sensors.map((sensor) => ` · ${sensor.value}`).join("")}
            {online.relay && ` · ${online.relay}`}
          </p>
        </>
      ) : (
        <p className="text-sm text-muted-foreground">
          {view.kind === "offline"
            ? t`The BELABOX isn’t connected to BELABOX Cloud. Check that it’s on and online.`
            : view.kind === "invalid_key"
              ? t`BELABOX Cloud refused the secret URL. Check it in settings.`
              : view.kind === "loading" || view.kind === "connecting"
                ? t`Connecting to BELABOX Cloud…`
                : t`Connects to BELABOX Cloud while OBS is open with the browser source.`}
        </p>
      )}
    </DeviceColumn>
  );
}

export function MoblinSummary({
  status,
  now,
  paused,
  details,
}: {
  status: MoblinStatus | null | undefined;
  now: number;
  paused: boolean;
  details: ReactNode;
}) {
  const view = moblinView(status, now);
  const connected = view.kind === "connected" || view.kind === "paused" ? view.status : null;
  const keys = bondingKeys(connected?.bonding_connections ?? []);
  const state = connected
    ? connected.streaming
      ? t`Streaming`
      : t`Not streaming`
    : view.kind === "loading"
      ? t`Connecting…`
      : t`Not connected`;
  return (
    <DeviceColumn
      name="Moblin"
      state={state}
      paused={paused || view.kind === "paused"}
      details={details}
    >
      {connected ? (
        <>
          <p className="text-xs text-muted-foreground">
            <span className="font-display text-3xl font-bold text-foreground tabular-nums">
              {connected.bitrate_kbps != null ? formatKbps(connected.bitrate_kbps) : "—"}
            </span>{" "}
            <Trans>kbps sent</Trans>
          </p>
          <p className="text-xs text-muted-foreground">
            {connected.battery_level != null && t`Battery ${connected.battery_level}%`}
            {thermalLabel(connected.thermal) && ` · ${thermalLabel(connected.thermal)}`}
            {connected.scenes.find((scene) => scene.id === connected.scene_id)?.name &&
              ` · ${connected.scenes.find((scene) => scene.id === connected.scene_id)?.name}`}
          </p>
          <dl className="divide-y divide-divider">
            {connected.bonding_connections.map((connection, index) => (
              <div
                key={keys[index]}
                className="flex items-center justify-between gap-3 py-1.5 text-xs"
              >
                <dt className="truncate text-muted-foreground">{connection.name}</dt>
                <dd className="font-display tabular-nums">
                  {formatKbps(connection.kbps)} <Trans>kbps</Trans>
                </dd>
              </div>
            ))}
          </dl>
          {connected.bonding_connections.length === 0 && (
            <p className="text-xs text-muted-foreground">
              {connected.bonding ?? connected.bitrate}
            </p>
          )}
        </>
      ) : (
        <p className="text-sm text-muted-foreground">
          {view.kind === "loading"
            ? t`Connecting…`
            : t`Moblin isn’t connected. Check Settings → Remote control → Streamer in Moblin.`}
        </p>
      )}
    </DeviceColumn>
  );
}
