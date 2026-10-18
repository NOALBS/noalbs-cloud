import { Lock, RefreshCw, Wifi as WifiIcon } from "lucide-react";
import { type FormEvent, useState } from "react";
import type {
  BelaboxDetails,
  BelaboxHotspot,
  BelaboxWifiDevice,
  BelaboxWifiNetwork,
} from "@/api/types";
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
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { BelaboxConfig } from "@/hooks/use-belabox-config";
import { hotspotErrors, wifiBand, wifiJoinErrors } from "@/lib/belabox";
import { cn } from "@/lib/utils";
import { Field, Section, SignalBars } from "./field";

/** The BELABOX's WiFi adapters: join, leave and forget networks, or run a hotspot. */
export function WifiPanel({
  details,
  config,
  canManage,
}: {
  details: BelaboxDetails;
  config: BelaboxConfig;
  canManage: boolean;
}) {
  if (details.wifi.length === 0) {
    return (
      <p className="rounded-xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
        This BELABOX reports no WiFi adapter.
      </p>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {details.wifi.map((device) => (
        <Adapter key={device.id} device={device} config={config} canManage={canManage} />
      ))}
    </div>
  );
}

function Adapter({
  device,
  config,
  canManage,
}: {
  device: BelaboxWifiDevice;
  config: BelaboxConfig;
  canManage: boolean;
}) {
  const hotspot = device.hotspot;
  const title = `${device.interface}${device.hardware ? ` · ${device.hardware}` : ""}`;

  return (
    <Section
      title={title}
      description={hotspot ? "Running as a hotspot" : "Connected as a WiFi client"}
    >
      {hotspot ? (
        <HotspotSettings device={device} hotspot={hotspot} config={config} canManage={canManage} />
      ) : (
        <CurrentNetwork device={device} />
      )}
      {/* Under the network, where they fit on a phone. */}
      <div className="flex flex-wrap gap-2">
        {!hotspot && <NetworkPicker device={device} config={config} canManage={canManage} />}
        {device.supports_hotspot && <HotspotToggle device={device} config={config} />}
      </div>
    </Section>
  );
}

function HotspotToggle({ device, config }: { device: BelaboxWifiDevice; config: BelaboxConfig }) {
  const running = device.hotspot != null;
  if (running) {
    return (
      <Button
        size="sm"
        variant="outline"
        disabled={config.hotspot.isPending}
        onClick={() => config.hotspot.mutate({ device: device.id, active: false })}
      >
        Stop hotspot
      </Button>
    );
  }
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button size="sm" variant="outline" disabled={config.hotspot.isPending}>
          Start hotspot
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Start the hotspot on {device.interface}?</AlertDialogTitle>
          <AlertDialogDescription>
            The adapter leaves its WiFi network and other devices can connect to it instead. The
            BELABOX can’t use that WiFi to stream until you stop the hotspot.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            onClick={() => config.hotspot.mutate({ device: device.id, active: true })}
          >
            Start hotspot
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/** Just the network the adapter is on; the list of the others is a dialog of its own. */
function CurrentNetwork({ device }: { device: BelaboxWifiDevice }) {
  const current = device.networks.find((network) => network.connected);

  return (
    <div className="flex flex-wrap items-center gap-3">
      {current ? (
        <>
          <SignalBars signal={current.signal} />
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="flex items-center gap-1.5 text-sm font-semibold">
              <span className="truncate">{current.ssid}</span>
              {current.security && (
                <Lock className="size-3 shrink-0 text-muted-foreground" aria-label="Secured" />
              )}
            </span>
            <span className="text-[11px] text-muted-foreground">
              {[wifiBand(current.frequency), "connected"].filter(Boolean).join(" · ")}
            </span>
          </span>
        </>
      ) : (
        <span className="flex min-w-0 flex-1 items-center gap-3 text-sm text-muted-foreground">
          <WifiIcon className="size-3.5 shrink-0" aria-hidden />
          Not connected to a WiFi network
        </span>
      )}
    </div>
  );
}

/** The networks in range and the saved ones: connect, join, forget, add by name. */
function NetworkPicker({
  device,
  config,
  canManage,
}: {
  device: BelaboxWifiDevice;
  config: BelaboxConfig;
  canManage: boolean;
}) {
  return (
    <Dialog>
      <DialogTrigger asChild>
        <Button size="sm" variant="outline">
          Choose network…
        </Button>
      </DialogTrigger>
      <DialogContent className="flex max-h-[85dvh] flex-col gap-3 sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>WiFi networks</DialogTitle>
          <DialogDescription>
            {device.interface} · the list refreshes by itself; scan to look again.
          </DialogDescription>
        </DialogHeader>
        <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto pr-3">
          <Button
            size="sm"
            variant="outline"
            className="self-start"
            onClick={() => config.scanWifi.mutate()}
            disabled={config.scanWifi.isPending}
          >
            <RefreshCw className={cn("size-3.5", config.scanWifi.isPending && "animate-spin")} />
            Scan
          </Button>
          <ul className="flex flex-col divide-y divide-divider border-y border-divider">
            {device.networks.map((network) => (
              <NetworkRow
                key={network.ssid}
                device={device}
                network={network}
                config={config}
                canManage={canManage}
              />
            ))}
            {device.networks.length === 0 && (
              <li className="py-2 text-sm text-muted-foreground">
                No networks found. Scan again, or add one below.
              </li>
            )}
          </ul>
          {canManage && <HiddenNetwork device={device} config={config} />}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function NetworkRow({
  device,
  network,
  config,
  canManage,
}: {
  device: BelaboxWifiDevice;
  network: BelaboxWifiNetwork;
  config: BelaboxConfig;
  canManage: boolean;
}) {
  const [joining, setJoining] = useState(false);
  const busy = config.connectWifi.isPending || config.disconnectWifi.isPending;
  const saved = network.saved;

  return (
    <li className={cn("flex flex-col gap-2 py-2", !network.available && "opacity-70")}>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        {network.available ? (
          <SignalBars signal={network.signal} />
        ) : (
          <WifiIcon className="size-3.5 text-muted-foreground" aria-hidden />
        )}
        <span className="flex min-w-32 flex-1 flex-col">
          <span className="flex items-center gap-1.5 text-sm font-semibold">
            <span className="truncate">{network.ssid}</span>
            {network.security && (
              <Lock className="size-3 shrink-0 text-muted-foreground" aria-label="Secured" />
            )}
          </span>
          <span className="text-[11px] text-muted-foreground">
            {[
              network.available ? wifiBand(network.frequency) : "Out of range",
              network.connected ? "connected" : saved ? "saved" : null,
            ]
              .filter(Boolean)
              .join(" · ")}
          </span>
        </span>
        <div className="flex shrink-0 items-center gap-1.5">
          {network.connected && saved && (
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => config.disconnectWifi.mutate(saved)}
            >
              Disconnect
            </Button>
          )}
          {!network.connected && saved && network.available && (
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => config.connectWifi.mutate(saved)}
            >
              Connect
            </Button>
          )}
          {!saved && network.available && canManage && (
            <Button size="sm" variant="outline" onClick={() => setJoining((open) => !open)}>
              Join
            </Button>
          )}
          {saved && canManage && <Forget ssid={network.ssid} uuid={saved} config={config} />}
        </div>
      </div>
      {joining && (
        <JoinForm
          device={device}
          ssid={network.ssid}
          secured={Boolean(network.security)}
          config={config}
          onDone={() => setJoining(false)}
        />
      )}
    </li>
  );
}

function Forget({ ssid, uuid, config }: { ssid: string; uuid: string; config: BelaboxConfig }) {
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button size="sm" variant="ghost" disabled={config.forgetWifi.isPending}>
          Forget
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Forget {ssid}?</AlertDialogTitle>
          <AlertDialogDescription>
            The BELABOX deletes the saved password and stops joining this network by itself.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Keep it</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={() => config.forgetWifi.mutate(uuid)}>
            Forget network
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/** Joins (and saves) a network: the password, if it has one. */
function JoinForm({
  device,
  ssid,
  secured,
  config,
  onDone,
}: {
  device: BelaboxWifiDevice;
  ssid: string;
  secured: boolean;
  config: BelaboxConfig;
  onDone: () => void;
}) {
  const [password, setPassword] = useState("");
  const errors = wifiJoinErrors(ssid, password);
  const missing = secured && !password;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (errors.ssid || errors.password || missing) return;
    config.addWifi.mutate(
      { device: device.id, ssid, password: password || null },
      { onSuccess: onDone },
    );
  };

  return (
    <form onSubmit={submit} className="flex items-start gap-2">
      <Field label="Password" error={errors.password} className="flex-1">
        <Input
          type="password"
          autoComplete="new-password"
          value={password}
          onChange={(event) => setPassword(event.target.value)}
          placeholder={secured ? "WiFi password" : "Open network: no password"}
          aria-label={`Password for ${ssid}`}
          aria-invalid={Boolean(errors.password)}
        />
      </Field>
      <Button
        type="submit"
        size="sm"
        className="mt-[1.4rem]"
        disabled={config.addWifi.isPending || missing || Boolean(errors.password)}
      >
        Join
      </Button>
    </form>
  );
}

/** For a network that doesn't show up in the scan (hidden) or isn't in range yet. */
function HiddenNetwork({ device, config }: { device: BelaboxWifiDevice; config: BelaboxConfig }) {
  const [open, setOpen] = useState(false);
  const [ssid, setSsid] = useState("");
  const [password, setPassword] = useState("");
  const errors = wifiJoinErrors(ssid, password);

  if (!open) {
    return (
      <Button size="sm" variant="ghost" className="self-start" onClick={() => setOpen(true)}>
        Add a network by name…
      </Button>
    );
  }

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (errors.ssid || errors.password) return;
    config.addWifi.mutate(
      { device: device.id, ssid, password: password || null },
      {
        onSuccess: () => {
          setOpen(false);
          setSsid("");
          setPassword("");
        },
      },
    );
  };

  return (
    <form onSubmit={submit} className="flex flex-col gap-3 sm:flex-row sm:items-start">
      <Field label="Network name" error={ssid ? errors.ssid : undefined} className="flex-1">
        <Input
          value={ssid}
          onChange={(event) => setSsid(event.target.value)}
          autoComplete="off"
          aria-label="Network name"
        />
      </Field>
      <Field
        label="Password"
        error={errors.password}
        hint="Leave empty for an open network."
        className="flex-1"
      >
        <Input
          type="password"
          autoComplete="new-password"
          value={password}
          onChange={(event) => setPassword(event.target.value)}
          aria-label="Password"
        />
      </Field>
      <div className="flex gap-2 sm:mt-[1.4rem]">
        <Button type="submit" size="sm" disabled={config.addWifi.isPending || Boolean(errors.ssid)}>
          Join
        </Button>
        <Button type="button" size="sm" variant="ghost" onClick={() => setOpen(false)}>
          Cancel
        </Button>
      </div>
    </form>
  );
}

function HotspotSettings({
  device,
  hotspot,
  config,
  canManage,
}: {
  device: BelaboxWifiDevice;
  hotspot: BelaboxHotspot;
  config: BelaboxConfig;
  canManage: boolean;
}) {
  const [name, setName] = useState(hotspot.name);
  const [password, setPassword] = useState("");
  const [channel, setChannel] = useState(hotspot.channel);
  const errors = hotspotErrors(name, password);
  const dirty = name !== hotspot.name || password !== "" || channel !== hotspot.channel;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (errors.name || errors.password) return;
    config.hotspotConfig.mutate(
      { device: device.id, name, password: password || null, channel },
      { onSuccess: () => setPassword("") },
    );
  };

  return (
    <form onSubmit={submit} className="flex flex-col gap-3">
      {hotspot.warnings.includes("modified") && (
        <p className="rounded-lg bg-low/12 px-2.5 py-1.5 text-xs text-low">
          The hotspot’s settings were changed outside the BELABOX software. Saving here resets them.
        </p>
      )}
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="Name" error={errors.name}>
          <Input
            value={name}
            onChange={(event) => setName(event.target.value)}
            disabled={!canManage}
            aria-label="Hotspot name"
          />
        </Field>
        <Field
          label="Password"
          error={errors.password}
          hint={canManage ? "Leave empty to keep the current one." : "Only admins can change it."}
        >
          <Input
            type="password"
            autoComplete="new-password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            placeholder={hotspot.has_password ? "Unchanged" : ""}
            disabled={!canManage}
            aria-label="Hotspot password"
          />
        </Field>
        <Field label="Band">
          <Select value={channel} onValueChange={setChannel} disabled={!canManage}>
            <SelectTrigger className="w-full" aria-label="Hotspot band">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {hotspot.channels.map((choice) => (
                <SelectItem key={choice.id} value={choice.id}>
                  {choice.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
      </div>
      {canManage && (
        <Button
          type="submit"
          size="sm"
          className="self-start"
          disabled={!dirty || config.hotspotConfig.isPending}
        >
          {config.hotspotConfig.isPending ? "Restarting hotspot…" : "Save hotspot"}
        </Button>
      )}
    </form>
  );
}
