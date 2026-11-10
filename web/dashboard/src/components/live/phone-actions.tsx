import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { Play, Square, Wrench } from "lucide-react";
import { useState } from "react";
import type { SwitcherStatus } from "@/api/types";
import { Button } from "@/components/ui/button";
import { SheetClose } from "@/components/ui/sheet";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import { PhoneSheet } from "./phone";
import { MarkerButton, type ObsControls } from "./quick-actions";

const SLOT =
  "flex min-h-14 min-w-0 flex-col items-center justify-center gap-1 rounded-xl px-1 text-[11px] font-medium disabled:opacity-40";

export function PhoneStreamButton({
  controls,
  status,
  disabled,
}: {
  controls: ObsControls;
  status: SwitcherStatus | null | undefined;
  disabled: boolean;
}) {
  const [open, setOpen] = useState(false);
  const streaming = Boolean(status?.streaming);
  const Icon = streaming ? Square : Play;
  return (
    <PhoneSheet
      open={open}
      onOpenChange={setOpen}
      title={streaming ? t`Stop the stream?` : t`Start the stream?`}
      description={
        streaming
          ? t`Your viewers will see the stream end.`
          : t`OBS goes live with the scene on air now${status?.current_scene ? ` (${status.current_scene.name})` : ""}.`
      }
      trigger={
        <button
          type="button"
          disabled={disabled || controls.stream.isPending}
          className={cn(
            SLOT,
            streaming ? "bg-offline/10 text-destructive-foreground" : "bg-primary/10 text-primary",
          )}
        >
          <Icon className="size-4" />
          <span>{streaming ? t`Stop` : t`Start`}</span>
        </button>
      }
    >
      <div className="flex flex-col gap-2">
        <Button
          variant={streaming ? "destructive" : "default"}
          disabled={disabled || controls.stream.isPending}
          onClick={() => {
            controls.stream.mutate(!streaming);
            setOpen(false);
          }}
        >
          {streaming ? t`Stop stream` : t`Start stream`}
        </Button>
        <SheetClose asChild>
          <Button variant="outline">{streaming ? t`Keep streaming` : t`Not yet`}</Button>
        </SheetClose>
      </div>
    </PhoneSheet>
  );
}

export function PhoneActions({
  controls,
  status,
  disabled,
  canControl,
  autoSwitchSetting,
}: {
  controls: ObsControls;
  status: SwitcherStatus | null | undefined;
  disabled: boolean;
  canControl: boolean;
  autoSwitchSetting: boolean;
}) {
  const auto = status ? status.auto_switch : autoSwitchSetting;
  return (
    <section
      aria-label={t`Live controls`}
      className="fixed inset-x-3 bottom-[calc(4.25rem+1px+env(safe-area-inset-bottom)+0.5rem)] z-30 grid grid-cols-4 gap-1 rounded-2xl border bg-card/95 p-1 shadow-lg backdrop-blur"
    >
      <PhoneStreamButton controls={controls} status={status} disabled={disabled} />
      <button
        type="button"
        disabled={disabled || controls.fix.isPending}
        onClick={() => controls.fix.mutate()}
        className={SLOT}
      >
        <Wrench className="size-4" />
        <Trans>Fix feed</Trans>
      </button>
      <MarkerButton
        controls={controls}
        status={status}
        disabled={disabled}
        variant="compact"
        className="h-auto min-h-14 flex-col gap-1 rounded-xl border-0 bg-transparent px-1 text-[11px] font-medium"
      />
      <div className={SLOT}>
        <Switch
          checked={auto}
          disabled={!canControl || controls.autoSwitch.isPending}
          onCheckedChange={(value) => controls.autoSwitch.mutate(value)}
          aria-label={t`Auto switch`}
        />
        <span>
          <Trans>Auto</Trans>
        </span>
      </div>
    </section>
  );
}
