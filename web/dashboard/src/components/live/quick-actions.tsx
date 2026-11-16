import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Bookmark, Circle, Play, Square, Wrench } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import type { SwitcherStatus } from "@/api/types";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button, buttonVariants } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Switch } from "@/components/ui/switch";
import { useNow } from "@/hooks/use-now";
import { errorMessage } from "@/lib/errors";
import { formatOffset } from "@/lib/history";
import { formatUptime } from "@/lib/live-format";
import { cn } from "@/lib/utils";
import { HoldButton } from "./hold-button";

type Path = { workspace_id: string; switcher_id: string };

/** The remote-control actions for one switcher, as mutations. */
export function useObsControls(path: Path) {
  const queryClient = useQueryClient();
  const onError = (error: unknown) => toast.error(errorMessage(error));

  const stream = useMutation({
    mutationFn: (active: boolean) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/stream", {
          params: { path },
          body: { active },
        }),
      ),
    onError,
  });
  const record = useMutation({
    mutationFn: (active: boolean) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/record", {
          params: { path },
          body: { active },
        }),
      ),
    onError,
  });
  const autoSwitch = useMutation({
    mutationFn: (enabled: boolean) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/auto-switch", {
          params: { path },
          body: { enabled },
        }),
      ),
    onError,
  });
  const fix = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/fix", {
          params: { path },
        }),
      ),
    onSuccess: ({ scene, sources }) =>
      sources.length > 0
        ? toast.success(t`Reconnecting ${sources.join(", ")}`)
        : toast.info(t`Nothing to fix: no stream source is playing on ${scene}`),
    onError,
  });

  const marker = useMutation({
    mutationFn: (description: string) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/markers", {
          params: { path },
          body: { description },
        }),
      ),
    onSuccess: ({ offset_seconds }) => {
      toast.success(t`Marker added at ${formatOffset(offset_seconds)}`);
      queryClient.invalidateQueries({
        queryKey: queryKeys.streams(path.workspace_id, path.switcher_id),
      });
    },
    onError,
  });

  return { stream, record, autoSwitch, fix, marker };
}

export type ObsControls = ReturnType<typeof useObsControls>;

interface ControlProps {
  controls: ObsControls;
  status: SwitcherStatus | null | undefined;
  /** No permission, or OBS isn't connected. */
  disabled: boolean;
  /** `tile` for the phone grid, `compact` for the desktop header. */
  variant: "tile" | "compact";
}

const VARIANT = {
  tile: "min-h-15 justify-start bg-surface-2 px-3 text-left",
  compact: "h-8 px-3",
};
const BASE = buttonVariants({ variant: "outline" });

export function StreamButton({ controls, status, disabled, variant }: ControlProps) {
  const { stream } = controls;
  const className = cn(BASE, VARIANT[variant]);

  // Confirmed like stopping: a stray tap on a phone in a pocket shouldn't go live.
  if (!status?.streaming) {
    return (
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button
            variant="ghost"
            type="button"
            disabled={disabled || stream.isPending}
            className={cn(className, "border-primary bg-primary text-primary-foreground")}
          >
            <Play className="size-4" />
            <span>
              <Trans>Start stream</Trans>
              {variant === "tile" && (
                <span className="block text-[11px] font-normal opacity-70">
                  <Trans>asks to confirm</Trans>
                </span>
              )}
            </span>
          </Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              <Trans>Start the stream?</Trans>
            </AlertDialogTitle>
            <AlertDialogDescription>
              <Trans>
                OBS goes live with the scene on air now
                {status?.current_scene ? ` (${status.current_scene.name})` : ""}.
              </Trans>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>
              <Trans>Not yet</Trans>
            </AlertDialogCancel>
            <AlertDialogAction onClick={() => stream.mutate(true)}>
              <Trans>Start stream</Trans>
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    );
  }

  return (
    <HoldButton
      disabled={disabled || stream.isPending}
      onComplete={() => stream.mutate(false)}
      className={cn(className, "border-offline/50 bg-offline/10 text-destructive-foreground")}
    >
      <Square className="size-4" />
      <span>
        <Trans>Stop stream</Trans>
        {variant === "tile" && (
          <span className="block text-[11px] font-normal opacity-70">
            <Trans>hold 1 second</Trans>
          </span>
        )}
      </span>
      <small className="hold-hint">hold</small>
    </HoldButton>
  );
}

export function RecordButton({ controls, status, disabled, variant }: ControlProps) {
  const now = useNow();
  const { record } = controls;
  const recording = Boolean(status?.recording);
  const paused = Boolean(status?.recording_paused);

  const content = (
    <>
      <Circle
        className={cn(
          "size-2.5",
          recording
            ? paused
              ? "fill-yellow-500 text-yellow-500"
              : "fill-offline text-offline"
            : "fill-idle text-idle",
        )}
      />
      <span>
        {variant === "tile" ? (
          <>
            <span className="block">
              <Trans>Record</Trans>
            </span>
            <span className="block text-[11px] font-normal text-muted-foreground">
              {recording
                ? paused
                  ? t`paused`
                  : status?.recording_started_at
                    ? formatUptime(status.recording_started_at, now)
                    : t`recording`
                : t`off`}
            </span>
          </>
        ) : recording ? (
          paused ? (
            t`REC paused`
          ) : (
            `REC ${status?.recording_started_at ? formatUptime(status.recording_started_at, now) : ""}`
          )
        ) : (
          t`Record`
        )}
      </span>
    </>
  );
  const className = cn(BASE, VARIANT[variant], variant === "compact" && "bg-card");
  if (!recording)
    return (
      <Button
        variant="ghost"
        type="button"
        disabled={disabled || record.isPending}
        onClick={() => record.mutate(true)}
        className={className}
      >
        {content}
      </Button>
    );
  return (
    <HoldButton
      aria-label="Stop recording"
      aria-pressed
      disabled={disabled || record.isPending}
      onComplete={() => record.mutate(false)}
      className={className}
    >
      {content}
      <small className="hold-hint">hold</small>
    </HoldButton>
  );
}

/** Reconnects the stream sources on the scene on air (`!fix`): unfreezes a stuck feed. */
export function FixButton({ controls, disabled, variant }: Omit<ControlProps, "status">) {
  const { fix } = controls;

  return (
    <Button
      variant="ghost"
      type="button"
      disabled={disabled || fix.isPending}
      onClick={() => fix.mutate()}
      title={t`Reconnect the stream sources playing on the scene on air`}
      className={cn(BASE, VARIANT[variant], variant === "compact" && "bg-card")}
    >
      <Wrench className={cn("size-4", fix.isPending && "animate-pulse")} />
      <span>
        <Trans>Fix feed</Trans>
        {variant === "tile" && (
          <span className="block text-[11px] font-normal text-muted-foreground">
            <Trans>reconnect sources</Trans>
          </span>
        )}
      </span>
    </Button>
  );
}

/** Marks this moment on the running stream, with an optional note (`!marker`). */
export function MarkerButton({
  controls,
  status,
  disabled,
  variant,
  className,
  markerCount,
}: ControlProps & { className?: string; markerCount?: number }) {
  const { marker } = controls;
  const [open, setOpen] = useState(false);
  const [description, setDescription] = useState("");
  const streaming = Boolean(status?.streaming);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="ghost"
          type="button"
          disabled={disabled || !streaming}
          title={streaming ? t`Mark this moment on the stream` : t`Markers go on a running stream`}
          className={cn(BASE, VARIANT[variant], variant === "compact" && "bg-card", className)}
        >
          <Bookmark className="size-4" />
          <span>
            <Trans>Marker</Trans>
            {variant === "tile" && (
              <span className="block text-[11px] font-normal text-muted-foreground">
                {markerCount == null ? t`Mark this moment` : t`${markerCount} this stream`}
              </span>
            )}
          </span>
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[min(20rem,calc(100vw-2rem))]">
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            marker.mutate(description, {
              onSuccess: () => {
                setDescription("");
                setOpen(false);
              },
            });
          }}
        >
          <Input
            autoFocus
            value={description}
            onChange={(event) => setDescription(event.target.value)}
            maxLength={200}
            placeholder={t`What happened? (optional)`}
            aria-label={t`Marker description`}
          />
          <Button type="submit" size="sm" disabled={marker.isPending}>
            <Trans>Add marker</Trans>
          </Button>
        </form>
      </PopoverContent>
    </Popover>
  );
}

/** The auto switch toggle, as a row with the current mode underneath. */
export function AutoSwitchToggle({
  controls,
  status,
  autoSwitchSetting,
  onlyWhileStreaming,
  canControl,
  className,
}: {
  controls: ObsControls;
  status: SwitcherStatus | null | undefined;
  /** The saved setting, used while no browser source reports the live value. */
  autoSwitchSetting: boolean;
  /** The "Only while streaming" setting: on, nothing switches until OBS streams. */
  onlyWhileStreaming: boolean;
  canControl: boolean;
  className?: string;
}) {
  const { autoSwitch } = controls;
  const on = status ? status.auto_switch : autoSwitchSetting;
  // Otherwise a test with the encoder off and OBS not streaming looks broken.
  const waiting = on && onlyWhileStreaming && status?.obs_connected && !status.streaming;

  return (
    <div
      className={cn(
        "flex min-h-14 items-center justify-between gap-3 rounded-xl border bg-sunken px-3.5 py-2 text-left",
        className,
      )}
    >
      <span className="flex flex-col gap-0.5">
        <span className="text-sm font-semibold">
          <Trans>Auto switch</Trans>
        </span>
        <span className="text-xs text-muted-foreground">
          {waiting
            ? t`Waiting: switches once OBS is streaming`
            : on
              ? t`Switching scenes on bitrate & RTT`
              : t`Paused: manual scene switching only`}
        </span>
      </span>
      <Switch
        checked={on}
        onCheckedChange={(enabled) => autoSwitch.mutate(enabled)}
        disabled={!canControl || autoSwitch.isPending}
        aria-label={t`Auto switch`}
      />
    </div>
  );
}
