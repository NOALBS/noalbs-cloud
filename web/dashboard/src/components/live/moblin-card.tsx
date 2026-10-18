import { useMutation } from "@tanstack/react-query";
import {
  Battery,
  BatteryLow,
  Circle,
  EyeOff,
  Flame,
  Mic,
  MicOff,
  Minus,
  Play,
  Plus,
  Square,
} from "lucide-react";
import { useId } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import type { MoblinConnection, MoblinStatus, MoblinStatusText, MoblinThermal } from "@/api/types";
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
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { errorMessage } from "@/lib/errors";
import { formatKbps } from "@/lib/live-format";
import {
  batteryLabel,
  bondingRows,
  canTurnOffConnection,
  MOBLIN_MAX_PRIORITY,
  MOBLIN_MIN_PRIORITY,
  moblinkRelays,
  moblinView,
  sortedPresets,
  stepZoom,
  systemMonitorLabel,
  thermalLabel,
  zoomLabel,
} from "@/lib/moblin";
import { cn } from "@/lib/utils";

type Path = { workspace_id: string; switcher_id: string };

/** Moblin's flame: white when fine, yellow when getting hot, red when hot. */
const FLAME_TONE: Record<MoblinThermal, string> = {
  normal: "text-foreground",
  serious: "text-low",
  critical: "text-offline",
};

/** Moblin remote control, as mutations. The status shows the outcome. */
function useMoblinControls(path: Path) {
  const onError = (error: unknown) => toast.error(errorMessage(error));

  const stream = useMutation({
    mutationFn: (active: boolean) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/stream", {
          params: { path },
          body: { active },
        }),
      ),
    onError,
  });
  const record = useMutation({
    mutationFn: (active: boolean) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/record", {
          params: { path },
          body: { active },
        }),
      ),
    onError,
  });
  const mute = useMutation({
    mutationFn: (muted: boolean) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/mute", {
          params: { path },
          body: { muted },
        }),
      ),
    onError,
  });
  const preset = useMutation({
    mutationFn: (id: string) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/bitrate-preset", {
          params: { path },
          body: { id },
        }),
      ),
    onError,
  });
  const scene = useMutation({
    mutationFn: (id: string) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/scene", {
          params: { path },
          body: { id },
        }),
      ),
    onError,
  });

  const zoom = useMutation({
    mutationFn: (x: number) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/zoom", {
          params: { path },
          body: { x },
        }),
      ),
    onError,
  });
  const zoomPreset = useMutation({
    mutationFn: (id: string) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/zoom-preset", {
          params: { path },
          body: { id },
        }),
      ),
    onError,
  });
  const stealth = useMutation({
    mutationFn: (on: boolean) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/stealth-mode", {
          params: { path },
          body: { on },
        }),
      ),
    onError,
  });
  const priorities = useMutation({
    mutationFn: (enabled: boolean) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/connection-priorities", {
          params: { path },
          body: { enabled },
        }),
      ),
    onError,
  });
  const connection = useMutation({
    mutationFn: ({ id, enabled, priority }: { id: string; enabled: boolean; priority?: number }) =>
      unwrap(
        api.PUT(
          "/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/connections/{connection_id}",
          {
            params: { path: { ...path, connection_id: id } },
            body: { enabled, priority: priority ?? null },
          },
        ),
      ),
    onError,
  });

  return {
    stream,
    record,
    mute,
    preset,
    scene,
    zoom,
    zoomPreset,
    stealth,
    priorities,
    connection,
  };
}

type Controls = ReturnType<typeof useMoblinControls>;

/**
 * The Moblin connected as a remote control streamer: live or not, what it sends,
 * SRTLA bonding, battery and heat, and for people who may control the switcher
 * go live/stop, record, mute, bitrate preset and scene. Shown only for switchers
 * with a Moblin password.
 */
export function MoblinCard({
  status,
  now,
  path,
  canControl,
}: {
  status: MoblinStatus | null | undefined;
  now: number;
  path: Path;
  canControl: boolean;
}) {
  const controls = useMoblinControls(path);
  const view = moblinView(status, now);
  const connected = view.kind === "connected" || view.kind === "paused" ? view.status : null;
  const controllable = canControl && view.kind === "connected";

  const chip = connected
    ? connected.streaming
      ? { label: "LIVE", tone: "text-live", fill: "bg-live" }
      : {
          label: view.kind === "paused" ? "PAUSED" : "READY",
          tone: "text-muted-foreground",
          fill: "bg-idle",
        }
    : view.kind === "loading"
      ? { label: "CONNECTING…", tone: "text-muted-foreground", fill: "bg-idle" }
      : { label: "NOT CONNECTED", tone: "text-muted-foreground", fill: "bg-idle" };

  return (
    <section aria-label="Moblin" className="flex flex-col gap-3 rounded-2xl border bg-card p-3.5">
      <div className="flex items-center justify-between gap-2">
        <span className="flex min-w-0 items-center gap-2">
          <span className="text-sm font-semibold">Moblin</span>
          <span
            className={cn(
              "flex items-center gap-1.5 text-[11px] font-bold tracking-wider",
              chip.tone,
            )}
          >
            <span className={cn("size-1.5 rounded-full", chip.fill)} />
            {chip.label}
          </span>
        </span>
        {connected && controllable && (
          <StreamToggle controls={controls} streaming={connected.streaming} />
        )}
      </div>

      {view.kind === "disconnected" && (
        <p className="text-sm text-muted-foreground">
          Moblin isn’t connected. Check Settings → Remote control → Streamer in Moblin.
        </p>
      )}

      {connected && <Details status={connected} controls={controls} canControl={controllable} />}
    </section>
  );
}

function Details({
  status,
  controls,
  canControl,
}: {
  status: MoblinStatus;
  controls: Controls;
  canControl: boolean;
}) {
  // [name, value, whether it's a problem]; the long lines (bonding, Moblink) have their own lists.
  const facts = (
    [
      ["Battery", batteryLabel(status), false],
      ["Heat", thermalLabel(status.thermal), false],
      ["Phone", status.system_monitor ? systemMonitorLabel(status.system_monitor) : null, false],
      ["DJI", status.dji_devices?.text ?? null, status.dji_devices?.ok === false],
    ] satisfies [string, string | null, boolean][]
  ).filter((fact): fact is [string, string, boolean] => fact[1] != null);

  return (
    <>
      <div className="flex flex-col">
        <span className="font-mono text-xl font-semibold tabular">
          {status.streaming ? (status.bitrate ?? "—") : "Not live"}
        </span>
        {status.streaming && status.uptime && (
          <span className="text-xs text-muted-foreground">Live for {status.uptime}</span>
        )}
      </div>

      {facts.length > 0 && (
        <dl className="grid grid-cols-2 gap-x-3 gap-y-1.5 text-xs">
          {facts.map(([name, value, problem]) => (
            <div
              key={name}
              className={cn("flex min-w-0 flex-col", value.length > 18 && "col-span-2")}
            >
              <dt className="text-muted-foreground">{name}</dt>
              <dd
                className={cn(
                  "font-mono break-words tabular",
                  problem && "text-destructive-foreground",
                )}
              >
                {name === "Heat" && status.thermal && (
                  <Flame
                    aria-hidden
                    className={cn("mr-1 inline size-3.5 align-[-2px]", FLAME_TONE[status.thermal])}
                  />
                )}
                {value}
              </dd>
            </div>
          ))}
        </dl>
      )}
      <Bonding status={status} />
      {status.moblink && <Moblink line={status.moblink} />}

      {canControl && (
        <div className="flex flex-wrap gap-2">
          <Button
            size="sm"
            variant={status.recording ? "secondary" : "outline"}
            onClick={() => controls.record.mutate(!status.recording)}
            disabled={controls.record.isPending}
            aria-pressed={status.recording}
          >
            <Circle className={cn("size-3.5", status.recording && "fill-offline text-offline")} />
            {status.recording ? "Recording" : "Record"}
          </Button>
          <Button
            size="sm"
            variant={status.muted ? "secondary" : "outline"}
            onClick={() => controls.mute.mutate(!status.muted)}
            disabled={controls.mute.isPending}
            aria-pressed={status.muted}
          >
            {status.muted ? <MicOff className="size-3.5" /> : <Mic className="size-3.5" />}
            {status.muted ? "Muted" : "Mute"}
          </Button>
          <Button
            size="sm"
            variant={status.stealth_mode ? "secondary" : "outline"}
            onClick={() => controls.stealth.mutate(!status.stealth_mode)}
            disabled={controls.stealth.isPending}
            aria-pressed={status.stealth_mode}
            title="Moblin’s screen shows a black image instead of the camera. The stream isn’t affected."
          >
            <EyeOff className="size-3.5" />
            {status.stealth_mode ? "Stealth on" : "Stealth"}
          </Button>
        </div>
      )}
      {!canControl && (status.recording || status.muted || status.stealth_mode) && (
        <p className="text-xs text-muted-foreground">
          {[
            status.recording && "Recording",
            status.muted && "Muted",
            status.stealth_mode && "Stealth mode",
          ]
            .filter(Boolean)
            .join(" · ")}
        </p>
      )}

      {status.zoom != null && <Zoom status={status} controls={controls} canControl={canControl} />}

      <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-1 xl:grid-cols-2">
        <Choice
          label="Scene"
          value={status.scene_id}
          options={status.scenes.map((scene) => ({ id: scene.id, label: scene.name }))}
          onChange={(id) => controls.scene.mutate(id)}
          disabled={!canControl || controls.scene.isPending}
        />
        <Choice
          label="Bitrate preset"
          value={status.bitrate_preset_id}
          options={sortedPresets(status).map((preset) => ({
            id: preset.id,
            label: `${formatKbps(preset.kbps)} kbps`,
          }))}
          onChange={(id) => controls.preset.mutate(id)}
          disabled={!canControl || controls.preset.isPending}
        />
      </div>

      {status.connections.length > 0 && (
        <Connections status={status} controls={controls} canControl={canControl} />
      )}
    </>
  );
}

/** SRTLA bonding: each connection's share of what's sent and its RTT. */
function Bonding({ status }: { status: MoblinStatus }) {
  if (!status.bonding && !status.bonding_rtts) return null;
  const rows = bondingRows(status.bonding, status.bonding_rtts);

  return (
    <div className="flex flex-col gap-1 text-xs">
      <span className="text-muted-foreground">Bonding</span>
      {rows ? (
        <ul className="flex flex-col gap-1">
          {rows.map((row) => (
            <li
              key={row.name}
              className="grid grid-cols-[minmax(0,1fr)_4rem_3rem] items-center gap-2"
            >
              <span className="break-words font-semibold">{row.name}</span>
              <span className="flex items-center gap-1.5">
                <span className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
                  <span
                    className="block h-full rounded-full bg-live"
                    style={{ width: `${row.percent ?? 0}%` }}
                  />
                </span>
                <span className="w-8 text-right font-mono tabular">
                  {row.percent != null ? `${row.percent}%` : "—"}
                </span>
              </span>
              <span className="text-right font-mono text-muted-foreground tabular">
                {row.rttMs != null ? `${row.rttMs} ms` : "—"}
              </span>
            </li>
          ))}
        </ul>
      ) : (
        // Not in the shape we know: show Moblin's lines whole.
        <>
          {status.bonding && <p className="font-mono break-words">{status.bonding}</p>}
          {status.bonding_rtts && (
            <p className="font-mono break-words text-muted-foreground">{status.bonding_rtts}</p>
          )}
        </>
      )}
    </div>
  );
}

/** Moblink relays (other phones bonding for this one) with their battery. */
function Moblink({ line }: { line: MoblinStatusText }) {
  const relays = moblinkRelays(line.text);

  return (
    <div className="flex flex-col gap-1 text-xs">
      <span className="text-muted-foreground">Moblink</span>
      {relays ? (
        <ul className="flex flex-wrap gap-1.5">
          {relays.map((relay) => (
            <li
              key={relay.name}
              className={cn(
                "flex items-center gap-1.5 rounded-md border border-divider px-2 py-0.5",
                (relay.lowBattery || relay.hot) && "border-offline/50 text-destructive-foreground",
              )}
            >
              <span className="font-semibold">{relay.name}</span>
              {relay.hot && <Flame aria-label="Hot" className="size-3.5 text-offline" />}
              {relay.battery != null && (
                <span className="flex items-center gap-0.5 font-mono tabular">
                  {relay.lowBattery ? (
                    <BatteryLow className="size-3.5" />
                  ) : (
                    <Battery className="size-3.5" />
                  )}
                  {relay.battery}%
                </span>
              )}
            </li>
          ))}
        </ul>
      ) : (
        <p className={cn("font-mono break-words", !line.ok && "text-destructive-foreground")}>
          {line.text}
        </p>
      )}
    </div>
  );
}

/** The camera's zoom: presets, and a step in or out. */
function Zoom({
  status,
  controls,
  canControl,
}: {
  status: MoblinStatus;
  controls: Controls;
  canControl: boolean;
}) {
  const x = status.zoom ?? 1;
  const busy = controls.zoom.isPending || controls.zoomPreset.isPending;

  return (
    <div className="flex flex-col gap-1.5 rounded-xl border border-divider px-3 py-2">
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs tracking-wider text-muted-foreground uppercase">Zoom</span>
        <div className="flex items-center gap-1.5">
          {canControl && (
            <Button
              size="icon"
              variant="outline"
              className="size-8"
              onClick={() => controls.zoom.mutate(stepZoom(x, -1))}
              disabled={busy}
              aria-label="Zoom out"
            >
              <Minus className="size-3.5" />
            </Button>
          )}
          <span className="w-14 text-center font-mono text-sm font-semibold tabular">
            {zoomLabel(x)}
          </span>
          {canControl && (
            <Button
              size="icon"
              variant="outline"
              className="size-8"
              onClick={() => controls.zoom.mutate(stepZoom(x, 1))}
              disabled={busy}
              aria-label="Zoom in"
            >
              <Plus className="size-3.5" />
            </Button>
          )}
        </div>
      </div>
      {status.zoom_presets.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {status.zoom_presets.map((preset) => {
            const active = preset.id === status.zoom_preset_id;
            return (
              <Button
                key={preset.id}
                size="sm"
                variant={active ? "secondary" : "outline"}
                className="h-7 px-2.5 font-mono text-xs"
                onClick={() => controls.zoomPreset.mutate(preset.id)}
                disabled={!canControl || busy || active}
                aria-pressed={active}
              >
                {preset.name}
              </Button>
            );
          })}
        </div>
      )}
    </div>
  );
}

/**
 * Moblin's SRTLA bonding connections. Their on/off and priorities only apply while
 * connection priorities are on; off, Moblin uses every connection alike.
 */
function Connections({
  status,
  controls,
  canControl,
}: {
  status: MoblinStatus;
  controls: Controls;
  canControl: boolean;
}) {
  const enabled = status.connection_priorities_enabled;
  const id = useId();

  return (
    <div className="flex flex-col gap-1.5 rounded-xl border border-divider px-3 py-2">
      <div className="flex items-center justify-between gap-2">
        <Label
          htmlFor={id}
          className="text-xs font-normal tracking-wider text-muted-foreground uppercase"
        >
          Connection priorities
        </Label>
        <Switch
          id={id}
          checked={enabled}
          onCheckedChange={(next) => controls.priorities.mutate(next)}
          disabled={!canControl || controls.priorities.isPending}
        />
      </div>
      {!enabled && (
        <p className="text-xs text-muted-foreground">
          Off: Moblin uses every connection alike. Turn on to switch connections off or prefer one.
          Only SRTLA streams use these.
        </p>
      )}
      <ul className="flex flex-col divide-y divide-divider">
        {status.connections.map((connection) => (
          <ConnectionRow
            key={connection.id}
            connection={connection}
            controls={controls}
            canControl={canControl && enabled}
            canTurnOff={canTurnOffConnection(status.connections, connection.id)}
          />
        ))}
      </ul>
    </div>
  );
}

function ConnectionRow({
  connection,
  controls,
  canControl,
  canTurnOff,
}: {
  connection: MoblinConnection;
  controls: Controls;
  canControl: boolean;
  canTurnOff: boolean;
}) {
  const { connection: change } = controls;
  const setPriority = (priority: number) =>
    change.mutate({ id: connection.id, enabled: connection.enabled, priority });

  return (
    <li className={cn("flex items-center gap-2 py-1.5", !connection.enabled && "opacity-60")}>
      <span className="min-w-0 flex-1 truncate text-sm font-semibold" title={connection.name}>
        {connection.name}
      </span>
      <span className="flex items-center gap-1" title="Priority: higher is used more when stable">
        {canControl && (
          <Button
            size="icon"
            variant="ghost"
            className="size-7"
            onClick={() => setPriority(connection.priority - 1)}
            disabled={change.isPending || connection.priority <= MOBLIN_MIN_PRIORITY}
            aria-label={`Lower ${connection.name}'s priority`}
          >
            <Minus className="size-3" />
          </Button>
        )}
        <span className="w-6 text-center font-mono text-xs tabular">{connection.priority}</span>
        {canControl && (
          <Button
            size="icon"
            variant="ghost"
            className="size-7"
            onClick={() => setPriority(connection.priority + 1)}
            disabled={change.isPending || connection.priority >= MOBLIN_MAX_PRIORITY}
            aria-label={`Raise ${connection.name}'s priority`}
          >
            <Plus className="size-3" />
          </Button>
        )}
      </span>
      <Switch
        checked={connection.enabled}
        onCheckedChange={(enabled) => change.mutate({ id: connection.id, enabled })}
        disabled={!canControl || change.isPending || (connection.enabled && !canTurnOff)}
        aria-label={`${connection.enabled ? "Turn off" : "Turn on"} ${connection.name}`}
      />
    </li>
  );
}

/** A labelled picker for one of Moblin's scenes or presets. */
function Choice({
  label,
  value,
  options,
  onChange,
  disabled,
}: {
  label: string;
  value: string | null;
  options: { id: string; label: string }[];
  onChange: (id: string) => void;
  disabled: boolean;
}) {
  const id = useId();
  const known = options.some((option) => option.id === value);
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <Label htmlFor={id} className="text-xs font-normal text-muted-foreground">
        {label}
      </Label>
      <Select
        // An empty value shows the placeholder (a bitrate that matches no preset).
        value={known && value ? value : ""}
        onValueChange={onChange}
        disabled={disabled || options.length === 0}
      >
        <SelectTrigger id={id} className="w-full">
          <SelectValue placeholder={options.length === 0 ? "None" : "Other"} />
        </SelectTrigger>
        <SelectContent>
          {options.map((option) => (
            <SelectItem key={option.id} value={option.id}>
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}

function StreamToggle({ controls, streaming }: { controls: Controls; streaming: boolean }) {
  const { stream } = controls;
  if (!streaming) {
    return (
      <Button size="sm" onClick={() => stream.mutate(true)} disabled={stream.isPending}>
        <Play className="size-3.5" />
        Go live
      </Button>
    );
  }
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button size="sm" variant="outline" disabled={stream.isPending}>
          <Square className="size-3.5" />
          Stop
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Stop the Moblin stream?</AlertDialogTitle>
          <AlertDialogDescription>
            Moblin stops sending, so the stream server loses the feed.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Keep streaming</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={() => stream.mutate(false)}>
            Stop Moblin
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
