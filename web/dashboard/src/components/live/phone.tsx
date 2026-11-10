import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { ArrowDown, ArrowUp, ChevronRight, Settings2 } from "lucide-react";
import { type ComponentProps, type ReactNode, useState } from "react";
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
import { Switch } from "@/components/ui/switch";
import { belaboxView } from "@/lib/belabox";
import { formatOffset } from "@/lib/history";
import { formatKbps, formatUptime } from "@/lib/live-format";
import { moblinView } from "@/lib/moblin";
import { cn } from "@/lib/utils";
import { latest } from "@/live/reducer";
import { ActivityFeed } from "./activity-feed";
import { BitrateChart } from "./bitrate-chart";
import { lookFor } from "./feed-state";
import { type LiveHero, lastFreshAt } from "./hero";
import { type Availability, type PhoneSection, type PhoneSectionId, tileNames } from "./layout";

export function PhoneSheet({
  title,
  description,
  trigger,
  children,
  open,
  onOpenChange,
}: {
  title: string;
  description: string;
  trigger: ReactNode;
  children: ReactNode;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetTrigger asChild>{trigger}</SheetTrigger>
      <SheetContent
        side="bottom"
        className="max-h-[85dvh] gap-0 rounded-t-2xl pb-[env(safe-area-inset-bottom)]"
      >
        <SheetHeader className="shrink-0 pr-10">
          <SheetTitle>{title}</SheetTitle>
          <SheetDescription>{description}</SheetDescription>
        </SheetHeader>
        <div className="min-h-0 overflow-y-auto overscroll-contain px-4 pb-6">{children}</div>
      </SheetContent>
    </Sheet>
  );
}

export function PhoneStatus({
  state,
  status,
  activeServer,
  live,
  now,
  paused,
  record,
}: ComponentProps<typeof LiveHero> & { record?: ReactNode }) {
  const look = lookFor(status);
  const sample = activeServer && status?.obs_connected ? latest(live, activeServer.id) : undefined;
  const neutral = [
    "connecting",
    "reconnecting",
    "browserOffline",
    "obsUnreachable",
    "notStreaming",
  ].includes(state);
  const label =
    state === "connecting"
      ? t`Connecting…`
      : state === "reconnecting"
        ? t`Reconnecting…`
        : paused || state === "suspended"
          ? t`Paused`
          : state === "browserOffline" || state === "obsUnreachable"
            ? t`Offline`
            : state === "notStreaming"
              ? t`Not streaming`
              : state === "noFeed"
                ? "BRB"
                : state === "low"
                  ? "LOW"
                  : "LIVE";
  const fresh = state === "noFeed" ? lastFreshAt(live.history) : null;
  const headline =
    state === "connecting"
      ? t`Connecting…`
      : state === "browserOffline" || state === "obsUnreachable"
        ? t`OBS is offline`
        : state === "notStreaming"
          ? t`No feed yet`
          : state === "noFeed"
            ? fresh == null
              ? t`No feed`
              : t`No feed for ${formatOffset((now - fresh) / 1000)}`
            : null;
  return (
    <section
      aria-label={t`Stream status`}
      className={cn(
        "rounded-2xl border bg-card p-4 shadow-tile",
        state === "reconnecting" && "opacity-50",
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <span
            role="status"
            className={cn(
              "rounded px-1.5 py-0.5 text-[11px] font-semibold",
              paused
                ? "bg-low/10 text-low"
                : neutral
                  ? "bg-muted text-muted-foreground"
                  : `${look.soft} ${look.text}`,
            )}
          >
            {label}
          </span>
          {status?.streaming && status.stream_started_at && (
            <span className="font-display text-xs text-muted-foreground tabular-nums">
              {formatUptime(status.stream_started_at, now)}
            </span>
          )}
        </div>
        <div className="min-w-0 text-right">
          <p className="text-[10px] text-muted-foreground uppercase">
            {state === "notStreaming" ? t`In OBS` : t`On air`}
          </p>
          <p className="truncate text-xs font-semibold">{status?.current_scene?.name ?? "—"}</p>
        </div>
      </div>
      {headline ? (
        <p className="mt-3 font-display text-2xl font-bold">{headline}</p>
      ) : (
        <p className="mt-3 flex flex-wrap items-baseline gap-1.5 font-display tabular-nums">
          <span className="text-4xl font-bold">{sample ? formatKbps(sample.bitrate) : "—"}</span>
          <span className="mr-2 text-xs text-muted-foreground">
            <Trans>kbps</Trans>
          </span>
          <span className="text-lg font-semibold">
            {sample?.rtt != null ? Math.round(sample.rtt) : "—"}
          </span>
          <span className="text-xs text-muted-foreground">
            <Trans>ms RTT</Trans>
          </span>
        </p>
      )}
      <p className="mt-1 text-xs text-muted-foreground">
        {activeServer?.name ??
          (state === "connecting" ? t`Waiting for status` : t`No server in use`)}
      </p>
      {activeServer && (
        <BitrateChart
          points={live.history[activeServer.id] ?? []}
          now={now}
          low={null}
          offline={null}
          thresholds={false}
          tone={look.text}
          height={38}
          className="mt-2"
        />
      )}
      {record && <div className="mt-3 flex justify-end border-t border-divider pt-3">{record}</div>}
    </section>
  );
}

export function PhoneSections({
  sections,
  updateSections,
  children,
}: {
  sections: PhoneSection[];
  updateSections: (change: (sections: PhoneSection[]) => PhoneSection[]) => void;
  children: (id: PhoneSectionId) => ReactNode;
}) {
  const names = tileNames();
  const move = (id: PhoneSectionId, direction: number) =>
    updateSections((items) => {
      const from = items.findIndex((item) => item.id === id);
      const to = from + direction;
      const item = items[from];
      const neighbor = items[to];
      if (!item || !neighbor) return items;
      const next = [...items];
      next[from] = neighbor;
      next[to] = item;
      return next;
    });
  return (
    <>
      {sections
        .filter((section) => !section.hidden)
        .map(({ id }) => (
          <section key={id} aria-label={names[id]} data-phone-section={id} className="min-w-0">
            <h2 className="mb-2 text-xs font-medium text-muted-foreground">{names[id]}</h2>
            <div className="overflow-hidden rounded-2xl border bg-card px-3 shadow-tile">
              {children(id)}
            </div>
          </section>
        ))}
      <PhoneSheet
        title={t`Live settings`}
        description={t`Choose which sections appear and their order.`}
        trigger={
          <Button variant="ghost" size="sm" className="self-center">
            <Settings2 />
            <Trans>Live settings</Trans>
          </Button>
        }
      >
        <div className="divide-y divide-divider">
          {sections.map(({ id, hidden }, index) => (
            <div key={id} className="flex min-h-14 items-center gap-2">
              <span className="mr-auto text-sm font-medium">{names[id]}</span>
              <Button
                variant="ghost"
                size="icon"
                aria-label={t`Move ${names[id]} up`}
                disabled={index === 0}
                onClick={() => move(id, -1)}
              >
                <ArrowUp />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                aria-label={t`Move ${names[id]} down`}
                disabled={index === sections.length - 1}
                onClick={() => move(id, 1)}
              >
                <ArrowDown />
              </Button>
              <Switch
                checked={!hidden}
                aria-label={t`Show ${names[id]}`}
                onCheckedChange={(shown) =>
                  updateSections((items) =>
                    items.map((item) => (item.id === id ? { ...item, hidden: !shown } : item)),
                  )
                }
              />
            </div>
          ))}
        </div>
      </PhoneSheet>
    </>
  );
}

export function PhoneDevices({
  availability,
  belabox,
  moblin,
  now,
  belaboxCard,
  moblinCard,
  fallback,
}: {
  availability: Availability;
  belabox: BelaboxStatus | null | undefined;
  moblin: MoblinStatus | null | undefined;
  now: number;
  belaboxCard: ReactNode;
  moblinCard: ReactNode;
  fallback: ReactNode;
}) {
  const bela = belaboxView(belabox, now);
  const mob = moblinView(moblin, now);
  const devices = [
    {
      name: "BELABOX",
      available: availability.belabox,
      bitrate: bela.kind === "online" ? bela.status.total_kbps : null,
      state:
        bela.kind === "online"
          ? bela.status.streaming
            ? t`Streaming`
            : t`Not streaming`
          : bela.kind === "loading" || bela.kind === "connecting"
            ? t`Connecting…`
            : t`Not connected`,
      card: belaboxCard,
    },
    {
      name: "Moblin",
      available: availability.moblin,
      bitrate: mob.kind === "connected" || mob.kind === "paused" ? mob.status.bitrate_kbps : null,
      state:
        mob.kind === "paused"
          ? t`Paused`
          : mob.kind === "connected"
            ? mob.status.streaming
              ? t`Streaming`
              : t`Not streaming`
            : mob.kind === "loading"
              ? t`Connecting…`
              : t`Not connected`,
      card: moblinCard,
    },
  ];
  return (
    <div className="divide-y divide-divider">
      {devices
        .filter((device) => device.available)
        .map((device) => (
          <PhoneSheet
            key={device.name}
            title={device.name}
            description={t`Device status and controls`}
            trigger={
              <button
                type="button"
                className="flex min-h-16 w-full items-center gap-2 py-3 text-left"
              >
                <span className="mr-auto">
                  <span className="block text-sm font-semibold">{device.name}</span>
                  <span className="block text-xs text-muted-foreground">{device.state}</span>
                </span>
                <span className="font-display text-sm tabular-nums">
                  {device.bitrate != null ? formatKbps(device.bitrate) : "—"}{" "}
                  <span className="text-xs text-muted-foreground">
                    <Trans>kbps</Trans>
                  </span>
                </span>
                <ChevronRight className="size-4 text-muted-foreground" />
              </button>
            }
          >
            {device.card}
          </PhoneSheet>
        ))}
      {!devices.some((device) => device.available) && (
        <p className="py-4 text-sm text-muted-foreground">{fallback}</p>
      )}
    </div>
  );
}

export function PhoneActivity(props: ComponentProps<typeof ActivityFeed>) {
  const [open, setOpen] = useState(false);
  return (
    <div className="py-1">
      {!open && <ActivityFeed {...props} limit={3} />}
      <PhoneSheet
        open={open}
        onOpenChange={setOpen}
        title={t`All activity`}
        description={t`Scene switches and stream changes`}
        trigger={
          <Button variant="ghost" size="sm" className="w-full">
            <Trans>All activity</Trans>
            <ChevronRight />
          </Button>
        }
      >
        {open && <ActivityFeed {...props} />}
      </PhoneSheet>
    </div>
  );
}
