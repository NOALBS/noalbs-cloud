import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Minus, Pencil, Play, Plus, Square, X } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import type { BelaboxInterface, BelaboxStatus } from "@/api/types";
import { BelaboxSettingsDialog } from "@/components/belabox/settings-dialog";
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
import {
  BELABOX_MAX_BITRATE,
  BELABOX_MIN_BITRATE,
  belaboxView,
  canDisable,
  INTERFACE_LABEL_MAX,
  interfaceLabel,
  interfaceTitle,
  modemLabel,
  roundBitrate,
  stepBitrate,
  unmatchedLabels,
  withLabel,
} from "@/lib/belabox";
import { errorMessage } from "@/lib/errors";
import { formatKbps } from "@/lib/live-format";
import { cn } from "@/lib/utils";
import type { Point } from "@/live/reducer";
import { BitrateChart } from "./bitrate-chart";

type Path = { workspace_id: string; switcher_id: string };

/** BELABOX remote control, as mutations. The status shows the outcome. */
function useBelaboxControls(path: Path) {
  const onError = (error: unknown) => toast.error(errorMessage(error));

  const stream = useMutation({
    mutationFn: (active: boolean) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/stream", {
          params: { path },
          body: { active },
        }),
      ),
    onError,
  });
  const bitrate = useMutation({
    mutationFn: (max_br: number) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/bitrate", {
          params: { path },
          body: { max_br },
        }),
      ),
    onError,
  });
  const netif = useMutation({
    mutationFn: ({ name, enabled }: { name: string; enabled: boolean }) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/interfaces/{name}", {
          params: { path: { ...path, name } },
          body: { enabled },
        }),
      ),
    onError,
  });

  return { stream, bitrate, netif };
}

type Controls = ReturnType<typeof useBelaboxControls>;

/** Saves the names of the interfaces (all of them: the API replaces the saved ones). */
function useSaveLabels(path: Path) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (labels: Record<string, string>) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/interface-labels", {
          params: { path },
          body: { labels },
        }),
      ),
    onSuccess: (updated) =>
      queryClient.setQueryData(queryKeys.switcher(path.workspace_id, path.switcher_id), updated),
    onError: (error) => toast.error(errorMessage(error)),
  });
}

/**
 * The BELABOX behind BELABOX Cloud: its network interfaces (modems) with their
 * throughput, sensors and notifications, and start/stop, max bitrate and
 * interface switches for people who may control the switcher. Shown only for
 * switchers with a remote key.
 */
export function BelaboxCard({
  status,
  history,
  now,
  path,
  canControl,
  canConfigure,
  canManage,
  labels,
  settingsLink,
}: {
  status: BelaboxStatus | null | undefined;
  history: Record<string, Point[]>;
  now: number;
  path: Path;
  canControl: boolean;
  /** May open the settings dialog (WiFi, modems, stream settings): controllers, not NOALBS support. */
  canConfigure: boolean;
  /** May also join and forget networks and change the hotspot and the modems. */
  canManage: boolean;
  /** The names given to the interfaces, by interface key. */
  labels: Record<string, string>;
  settingsLink: { workspace: string; switcher: string };
}) {
  const controls = useBelaboxControls(path);
  const saveLabels = useSaveLabels(path);
  const view = belaboxView(status, now);
  const online = view.kind === "online" ? view.status : null;

  const chip = {
    loading: { label: "CONNECTING…", tone: "text-muted-foreground", fill: "bg-idle" },
    idle: { label: "NOT CONNECTED", tone: "text-muted-foreground", fill: "bg-idle" },
    connecting: { label: "CONNECTING…", tone: "text-muted-foreground", fill: "bg-idle" },
    invalid_key: { label: "KEY REFUSED", tone: "text-offline", fill: "bg-offline" },
    offline: { label: "OFFLINE", tone: "text-offline", fill: "bg-offline" },
    online: online?.streaming
      ? { label: "STREAMING", tone: "text-live", fill: "bg-live" }
      : { label: "READY", tone: "text-muted-foreground", fill: "bg-idle" },
  }[view.kind];

  return (
    <section aria-label="BELABOX" className="flex flex-col gap-3 rounded-2xl border bg-card p-3.5">
      <div className="flex items-center justify-between gap-2">
        <span className="flex min-w-0 items-center gap-2">
          <span className="text-sm font-semibold">BELABOX</span>
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
        {online && (
          <div className="flex shrink-0 items-center gap-1.5">
            {canConfigure && <BelaboxSettingsDialog path={path} canManage={canManage} />}
            {canControl && <StreamToggle controls={controls} streaming={online.streaming} />}
          </div>
        )}
      </div>

      {view.kind === "idle" && (
        <p className="text-sm text-muted-foreground">
          Connects to BELABOX Cloud while OBS is open with the browser source.
        </p>
      )}
      {view.kind === "connecting" && (
        <p className="text-sm text-muted-foreground">Connecting to BELABOX Cloud…</p>
      )}
      {view.kind === "offline" && (
        <p className="text-sm text-muted-foreground">
          The BELABOX isn’t connected to BELABOX Cloud. Check that it’s on and online.
        </p>
      )}
      {view.kind === "invalid_key" && (
        <p className="text-sm text-muted-foreground">
          BELABOX Cloud refused the secret URL.{" "}
          <Link
            to="/$workspace/s/$switcher/obs"
            params={settingsLink}
            className="font-semibold text-primary hover:underline"
          >
            Check it in the settings
          </Link>
        </p>
      )}

      {online && (
        <>
          <span className="font-mono text-xl font-semibold tabular">
            {formatKbps(online.total_kbps)}
            <span className="text-xs font-normal text-muted-foreground"> kbps sent</span>
          </span>
          <MaxBitrate
            controls={controls}
            maxBr={online.max_br ?? null}
            streaming={online.streaming}
            canControl={canControl}
          />

          <ul className="flex flex-col divide-y divide-divider border-y border-divider">
            {online.interfaces.map((netif) => (
              <InterfaceRow
                key={netif.name}
                netif={netif}
                points={history[netif.name] ?? []}
                now={now}
                controls={controls}
                canControl={canControl}
                canTurnOff={canDisable(online.interfaces, netif.name)}
                labels={labels}
                canRename={canManage}
                onRename={(label) =>
                  netif.key && saveLabels.mutate(withLabel(labels, netif.key, label))
                }
                renaming={saveLabels.isPending}
              />
            ))}
            {online.interfaces.length === 0 && (
              <li className="py-2 text-sm text-muted-foreground">
                No network interfaces reported.
              </li>
            )}
          </ul>

          {canManage && (
            <SavedNames
              names={unmatchedLabels(online.interfaces, labels)}
              pending={saveLabels.isPending}
              onForget={(key) => saveLabels.mutate(withLabel(labels, key, ""))}
            />
          )}

          {online.notifications.map((notice) => (
            <p
              key={notice.name}
              className={cn(
                "rounded-lg px-2.5 py-1.5 text-xs",
                notice.kind === "error"
                  ? "bg-offline/14 text-destructive-foreground"
                  : notice.kind === "warning"
                    ? "bg-low/12 text-low"
                    : "bg-live/12 text-live",
              )}
            >
              {notice.message}
            </p>
          ))}

          <dl className="grid grid-cols-2 gap-x-3 gap-y-1 text-xs">
            {[
              ...online.sensors.map((sensor) => [sensor.name, sensor.value] as const),
              ...(online.pipeline ? [["Pipeline", online.pipeline] as const] : []),
              ...(online.srt_latency != null
                ? [["SRT latency", `${formatKbps(online.srt_latency)} ms`] as const]
                : []),
              ...(online.relay ? [["Relay", online.relay] as const] : []),
            ].map(([name, value]) => (
              <div
                key={name}
                className={cn("flex min-w-0 flex-col", value.length > 18 && "col-span-2")}
              >
                <dt className="text-muted-foreground">{name}</dt>
                <dd className="font-mono break-words tabular">{value}</dd>
              </div>
            ))}
          </dl>
        </>
      )}
    </section>
  );
}

function StreamToggle({ controls, streaming }: { controls: Controls; streaming: boolean }) {
  const { stream } = controls;
  if (!streaming) {
    return (
      <Button size="sm" onClick={() => stream.mutate(true)} disabled={stream.isPending}>
        <Play className="size-3.5" />
        Start
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
          <AlertDialogTitle>Stop the BELABOX stream?</AlertDialogTitle>
          <AlertDialogDescription>
            The BELABOX stops sending, so the stream server loses the feed.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Keep streaming</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={() => stream.mutate(false)}>
            Stop BELABOX
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/**
 * BELABOX's max bitrate: − / + steps of 250 kbps, or type a value (set on Enter
 * or when leaving the field). While BELABOX streams it applies at once;
 * otherwise obs-link keeps it for the next start.
 */
function MaxBitrate({
  controls,
  maxBr,
  streaming,
  canControl,
}: {
  controls: Controls;
  maxBr: number | null;
  streaming: boolean;
  canControl: boolean;
}) {
  const { bitrate } = controls;
  // What's being typed; `null` shows the current value.
  const [draft, setDraft] = useState<string | null>(null);

  const set = (value: number) => {
    if (value !== maxBr) bitrate.mutate(value);
  };
  const commit = () => {
    if (draft === null) return;
    const value = roundBitrate(Number(draft));
    setDraft(null);
    if (value !== null) set(value);
  };

  return (
    <div className="flex flex-col gap-1.5 rounded-xl border border-divider px-3 py-2">
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs tracking-wider text-muted-foreground uppercase">Max bitrate</span>
        {canControl ? (
          <div className="flex items-center gap-1.5">
            <Button
              size="icon"
              variant="outline"
              className="size-8"
              aria-label="Lower the max bitrate"
              onClick={() => maxBr != null && set(stepBitrate(maxBr, -1))}
              disabled={bitrate.isPending || maxBr == null || maxBr <= BELABOX_MIN_BITRATE}
            >
              <Minus className="size-3.5" />
            </Button>
            <Input
              aria-label="Max bitrate in kbps"
              inputMode="numeric"
              value={draft ?? (maxBr != null ? String(maxBr) : "")}
              placeholder="kbps"
              onChange={(event) => setDraft(event.target.value.replace(/\D/g, ""))}
              onBlur={commit}
              onKeyDown={(event) => {
                if (event.key === "Enter") commit();
                if (event.key === "Escape") setDraft(null);
              }}
              disabled={bitrate.isPending}
              className="h-8 w-20 text-center font-mono tabular"
            />
            <Button
              size="icon"
              variant="outline"
              className="size-8"
              aria-label="Raise the max bitrate"
              onClick={() => maxBr != null && set(stepBitrate(maxBr, 1))}
              disabled={bitrate.isPending || maxBr == null || maxBr >= BELABOX_MAX_BITRATE}
            >
              <Plus className="size-3.5" />
            </Button>
            <span className="text-xs text-muted-foreground">kbps</span>
          </div>
        ) : (
          <span className="font-mono text-sm tabular">
            {maxBr != null ? `${formatKbps(maxBr)} kbps` : "—"}
          </span>
        )}
      </div>
      {canControl && !streaming && (
        <span className="text-[11px] text-muted-foreground">
          Used from the next start (here, chat or with the OBS stream).
        </span>
      )}
    </div>
  );
}

function InterfaceRow({
  netif,
  points,
  now,
  controls,
  canControl,
  canTurnOff,
  labels,
  canRename,
  onRename,
  renaming,
}: {
  netif: BelaboxInterface;
  points: Point[];
  now: number;
  controls: Controls;
  canControl: boolean;
  canTurnOff: boolean;
  labels: Record<string, string>;
  canRename: boolean;
  onRename: (label: string) => void;
  renaming: boolean;
}) {
  const { netif: toggle } = controls;
  const modem = modemLabel(netif);
  const label = interfaceLabel(netif, labels);
  const title = interfaceTitle(netif, labels);
  // On its own line under the row, so a long modem or error text wraps instead of being cut.
  // A named interface keeps its real name here: the name follows the device, not the port.
  const detail = [label ? netif.name : "", netif.error ?? (modem || (netif.enabled ? "" : "off"))]
    .filter(Boolean)
    .join(" · ");

  return (
    <li className={cn("flex flex-col gap-0.5 py-2", !netif.enabled && "opacity-60")}>
      <div className="flex items-center gap-3">
        <span
          className={cn(
            "w-20 shrink-0 text-sm font-semibold",
            label ? "break-words" : "font-mono break-all",
          )}
        >
          {canRename && netif.key ? (
            <RenameInterface
              title={title}
              label={label ?? ""}
              pending={renaming}
              onSave={onRename}
            />
          ) : (
            title
          )}
        </span>
        <BitrateChart
          points={points}
          now={now}
          low={null}
          offline={null}
          thresholds={false}
          tone={netif.enabled ? "text-live" : "text-muted-foreground"}
          height={28}
          className="min-w-0 flex-1"
        />
        <span className="w-14 shrink-0 text-right font-mono text-sm tabular">
          {formatKbps(netif.kbps)}
        </span>
        {canControl && (
          <Switch
            checked={netif.enabled}
            onCheckedChange={(enabled) => toggle.mutate({ name: netif.name, enabled })}
            disabled={toggle.isPending || (netif.enabled && !canTurnOff)}
            aria-label={`${netif.enabled ? "Turn off" : "Turn on"} ${netif.name}`}
          />
        )}
      </div>
      {detail && (
        <span
          className={cn(
            "text-[11px] break-words",
            netif.error ? "text-destructive-foreground" : "text-muted-foreground",
          )}
        >
          {detail}
        </span>
      )}
    </li>
  );
}

/** The interface's name, which opens a popover to give it another one. */
function RenameInterface({
  title,
  label,
  pending,
  onSave,
}: {
  title: string;
  label: string;
  pending: boolean;
  onSave: (label: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState(label);

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) setDraft(label);
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          title="Name this interface"
          className="group flex max-w-full items-center gap-1 text-left font-[inherit] hover:text-primary"
        >
          <span className="min-w-0">{title}</span>
          <Pencil className="size-3 shrink-0 text-muted-foreground group-hover:text-primary" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-[min(18rem,calc(100vw-2rem))]">
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            onSave(draft);
            setOpen(false);
          }}
        >
          <Input
            autoFocus
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            maxLength={INTERFACE_LABEL_MAX}
            placeholder="Name, e.g. Phone A"
            aria-label="Interface name"
          />
          <p className="text-xs text-muted-foreground">
            The name tries to follow the device when its port changes (usb0 → usb1), but the BELABOX
            doesn’t always let us tell devices apart, so it can end up on the wrong one, e.g. with
            two identical WiFi adapters or ethernet ports. Check the interface name shown under it.
            Leave it empty to remove the name.
          </p>
          <Button type="submit" size="sm" disabled={pending}>
            Save
          </Button>
        </form>
      </PopoverContent>
    </Popover>
  );
}

/** Names of devices that aren't connected now, so a name that no longer fits can be removed. */
function SavedNames({
  names,
  pending,
  onForget,
}: {
  names: { key: string; label: string }[];
  pending: boolean;
  onForget: (key: string) => void;
}) {
  if (names.length === 0) return null;
  return (
    <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
      <span>Names without a device:</span>
      {names.map(({ key, label }) => (
        <span
          key={key}
          className="inline-flex items-center gap-1 rounded-full border border-divider py-0.5 pr-1 pl-2"
        >
          {label}
          <button
            type="button"
            disabled={pending}
            onClick={() => onForget(key)}
            aria-label={`Remove the name ${label}`}
            className="rounded-full p-0.5 hover:bg-muted hover:text-foreground"
          >
            <X className="size-3" />
          </button>
        </span>
      ))}
    </div>
  );
}
