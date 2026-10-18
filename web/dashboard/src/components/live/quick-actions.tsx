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
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Switch } from "@/components/ui/switch";
import { useNow } from "@/hooks/use-now";
import { errorMessage } from "@/lib/errors";
import { formatOffset } from "@/lib/history";
import { formatUptime } from "@/lib/live-format";
import { cn } from "@/lib/utils";

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
        ? toast.success(`Reconnecting ${sources.join(", ")}`)
        : toast.info(`Nothing to fix: no stream source is playing on ${scene}`),
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
      toast.success(`Marker added at ${formatOffset(offset_seconds)}`);
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
  tile: "min-h-16 rounded-2xl px-4 text-[15px]",
  compact: "h-10 rounded-[10px] px-3.5 text-sm",
};
const BASE =
  "flex items-center justify-center gap-2.5 border font-semibold transition-colors disabled:opacity-50";

export function StreamButton({ controls, status, disabled, variant }: ControlProps) {
  const { stream } = controls;
  const className = cn(BASE, VARIANT[variant]);

  if (!status?.streaming) {
    return (
      <button
        type="button"
        disabled={disabled || stream.isPending}
        onClick={() => stream.mutate(true)}
        className={cn(className, "border-primary bg-primary text-primary-foreground")}
      >
        <Play className="size-4" />
        Start stream
      </button>
    );
  }

  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <button
          type="button"
          disabled={disabled || stream.isPending}
          className={cn(className, "border-offline/50 bg-offline/10 text-destructive-foreground")}
        >
          <Square className="size-4" />
          Stop stream
        </button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Stop the stream?</AlertDialogTitle>
          <AlertDialogDescription>Your viewers will see the stream end.</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Keep streaming</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={() => stream.mutate(false)}>
            Stop stream
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

export function RecordButton({ controls, status, disabled, variant }: ControlProps) {
  const now = useNow();
  const { record } = controls;
  const recording = Boolean(status?.recording);
  const paused = Boolean(status?.recording_paused);

  return (
    <button
      type="button"
      disabled={disabled || record.isPending}
      onClick={() => record.mutate(!recording)}
      aria-pressed={recording}
      className={cn(BASE, VARIANT[variant], "bg-card")}
    >
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
      <span className="font-mono tabular">
        {recording
          ? paused
            ? "REC paused"
            : `REC ${status?.recording_started_at ? formatUptime(status.recording_started_at, now) : ""}`
          : "Record"}
      </span>
    </button>
  );
}

/** Reconnects the stream sources on the scene on air (`!fix`): unfreezes a stuck feed. */
export function FixButton({ controls, disabled, variant }: Omit<ControlProps, "status">) {
  const { fix } = controls;

  return (
    <button
      type="button"
      disabled={disabled || fix.isPending}
      onClick={() => fix.mutate()}
      title="Reconnect the stream sources playing on the scene on air"
      className={cn(BASE, VARIANT[variant], "bg-card")}
    >
      <Wrench className={cn("size-4", fix.isPending && "animate-pulse")} />
      Fix feed
    </button>
  );
}

/** Marks this moment on the running stream, with an optional note (`!marker`). */
export function MarkerButton({
  controls,
  status,
  disabled,
  variant,
  className,
}: ControlProps & { className?: string }) {
  const { marker } = controls;
  const [open, setOpen] = useState(false);
  const [description, setDescription] = useState("");
  const streaming = Boolean(status?.streaming);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          disabled={disabled || !streaming}
          title={streaming ? "Mark this moment on the stream" : "Markers go on a running stream"}
          className={cn(BASE, VARIANT[variant], "bg-card", className)}
        >
          <Bookmark className="size-4" />
          Marker
        </button>
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
            placeholder="What happened? (optional)"
            aria-label="Marker description"
          />
          <Button type="submit" size="sm" disabled={marker.isPending}>
            Add marker
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
  canControl,
  className,
}: {
  controls: ObsControls;
  status: SwitcherStatus | null | undefined;
  /** The saved setting, used while no browser source reports the live value. */
  autoSwitchSetting: boolean;
  canControl: boolean;
  className?: string;
}) {
  const { autoSwitch } = controls;
  const on = status ? status.auto_switch : autoSwitchSetting;

  return (
    <div
      className={cn(
        "flex min-h-14 items-center justify-between gap-3 rounded-xl border bg-sunken px-3.5 py-2 text-left",
        className,
      )}
    >
      <span className="flex flex-col gap-0.5">
        <span className="text-sm font-semibold">Auto switch</span>
        <span className="text-xs text-muted-foreground">
          {on ? "Switching scenes on bitrate & RTT" : "Paused: manual scene switching only"}
        </span>
      </span>
      <Switch
        checked={on}
        onCheckedChange={(enabled) => autoSwitch.mutate(enabled)}
        disabled={!canControl || autoSwitch.isPending}
        aria-label="Auto switch"
      />
    </div>
  );
}
