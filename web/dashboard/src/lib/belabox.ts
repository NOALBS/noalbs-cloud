import type {
  BelaboxDetails,
  BelaboxInterface,
  BelaboxModem,
  BelaboxModemDetails,
  BelaboxResult,
  BelaboxSettingsUpdate,
  BelaboxStatus,
} from "@/api/types";

/** BELABOX's max bitrate range and step (its own slider moves by 250 kbps). */
export const BELABOX_MIN_BITRATE = 300;
export const BELABOX_MAX_BITRATE = 12000;
export const BELABOX_BITRATE_STEP = 250;

/** A status older than this is gone: obs-link rewrites it at least every 10 s. */
const STALE_MS = 45_000;

export type BelaboxView =
  /** Waiting for the live feed. */
  | { kind: "loading" }
  /** No link runs: the browser source isn't connected (or the key was just saved). */
  | { kind: "idle" }
  | { kind: "connecting" }
  | { kind: "invalid_key" }
  | { kind: "offline" }
  | { kind: "online"; status: BelaboxStatus };

/** What the BELABOX card should show. */
export function belaboxView(status: BelaboxStatus | null | undefined, now: number): BelaboxView {
  if (status === undefined) return { kind: "loading" };
  if (status === null || now - Date.parse(status.updated_at) > STALE_MS) return { kind: "idle" };
  switch (status.link) {
    case "connecting":
      return { kind: "connecting" };
    case "invalid_key":
      return { kind: "invalid_key" };
    case "encoder_offline":
      return { kind: "offline" };
    case "online":
      return { kind: "online", status };
  }
}

/** Whether `name` may be turned off: BELABOX refuses to turn off the last enabled one. */
export function canDisable(interfaces: BelaboxInterface[], name: string): boolean {
  return interfaces.some((netif) => netif.name !== name && netif.enabled);
}

/** One step up or down from `current`, on BELABOX's 250 kbps grid, within its range. */
export function stepBitrate(current: number, direction: 1 | -1): number {
  const snapped =
    direction > 0
      ? Math.floor(current / BELABOX_BITRATE_STEP) * BELABOX_BITRATE_STEP + BELABOX_BITRATE_STEP
      : Math.ceil(current / BELABOX_BITRATE_STEP) * BELABOX_BITRATE_STEP - BELABOX_BITRATE_STEP;
  return Math.min(BELABOX_MAX_BITRATE, Math.max(BELABOX_MIN_BITRATE, snapped));
}

/** "Vodafone · 4G · 72%" for a cellular modem. */
export function modemStatusLabel(modem: BelaboxModem): string {
  return [
    modem.network,
    modem.network_type,
    modem.signal != null ? `${modem.signal}%` : null,
    modem.roaming ? "roaming" : null,
  ]
    .filter(Boolean)
    .join(" · ");
}

/** The same for an interface; empty for ones that aren't a modem. */
export function modemLabel(netif: BelaboxInterface): string {
  return netif.modem ? modemStatusLabel(netif.modem) : "";
}

/** The longest name for an interface (the API refuses more). */
export const INTERFACE_LABEL_MAX = 30;

type InterfaceLabels = Record<string, string>;

/** The name the streamer gave an interface, if any: saved under its `key`, which follows the device. */
export function interfaceLabel(
  netif: BelaboxInterface,
  labels: InterfaceLabels,
): string | undefined {
  return netif.key ? labels[netif.key] : undefined;
}

/** What to call an interface: its name or the interface name (`usb0`). */
export function interfaceTitle(netif: BelaboxInterface, labels: InterfaceLabels): string {
  return interfaceLabel(netif, labels) ?? netif.name;
}

/** The saved names whose device isn't connected now (unplugged, or its key changed). */
export function unmatchedLabels(
  interfaces: BelaboxInterface[],
  labels: InterfaceLabels,
): { key: string; label: string }[] {
  const present = new Set(interfaces.map((netif) => netif.key));
  return Object.entries(labels)
    .filter(([key]) => !present.has(key))
    .map(([key, label]) => ({ key, label }));
}

/** The saved names with `key` named `label`, or without it when `label` is blank. */
export function withLabel(labels: InterfaceLabels, key: string, label: string): InterfaceLabels {
  const { [key]: _old, ...rest } = labels;
  const trimmed = label.trim();
  return trimmed ? { ...rest, [key]: trimmed } : rest;
}

/** A typed max bitrate on BELABOX's 250 kbps grid (like belabot), or `null` if it isn't one. */
export function roundBitrate(value: number): number | null {
  if (!Number.isFinite(value) || value <= 0) return null;
  const rounded = Math.round(value / BELABOX_BITRATE_STEP) * BELABOX_BITRATE_STEP;
  return Math.min(BELABOX_MAX_BITRATE, Math.max(BELABOX_MIN_BITRATE, rounded));
}

// --- Settings dialog: WiFi, modems, audio and stream settings -----------------

/** BELABOX's limits (belaUI's `validateConfig`); the API checks them too. */
export const AUDIO_DELAY_MIN = -2000;
export const AUDIO_DELAY_MAX = 2000;
export const SRT_LATENCY_MIN = 100;
export const SRT_LATENCY_MAX = 10_000;
export const WIFI_PASSWORD_MIN = 8;
export const WIFI_PASSWORD_MAX = 63;
export const HOTSPOT_PASSWORD_MAX = 64;
export const NAME_MAX_BYTES = 32;

const utf8Length = (text: string) => new TextEncoder().encode(text).length;

/** A WiFi or modem signal (0–100) as 0–4 bars. */
export function signalBars(signal: number | null | undefined): 0 | 1 | 2 | 3 | 4 {
  if (signal == null || signal <= 0) return 0;
  if (signal < 25) return 1;
  if (signal < 50) return 2;
  if (signal < 75) return 3;
  return 4;
}

/** "2.4 GHz" / "5 GHz" for a WiFi frequency in MHz. */
export function wifiBand(mhz: number | null | undefined): string {
  if (mhz == null || mhz <= 0) return "";
  if (mhz < 3000) return "2.4 GHz";
  if (mhz < 5900) return "5 GHz";
  return "6 GHz";
}

/** How a WiFi or hotspot request ended, as a message for a toast. */
export function describeResult(result: BelaboxResult): { ok: boolean; message: string } {
  const ok = result.error == null;
  switch (result.kind) {
    case "wifi_new":
      if (ok) return { ok, message: "Joined the network and saved it" };
      return {
        ok,
        message:
          result.error === "auth"
            ? "Wrong WiFi password"
            : "The BELABOX couldn't join that network",
      };
    case "wifi_connect":
      return {
        ok,
        message: ok ? "Connected to the network" : "The BELABOX couldn't connect to that network",
      };
    case "hotspot_config":
      if (ok) return { ok, message: "Hotspot updated" };
      return {
        ok,
        message:
          {
            name: "The BELABOX refused the hotspot name",
            password: "The BELABOX refused the hotspot password",
            channel: "The BELABOX refused the hotspot band",
          }[result.error ?? ""] ?? "The BELABOX couldn't apply the hotspot settings",
      };
  }
}

export interface SettingsForm {
  pipeline: string;
  audioSource: string;
  audioCodec: string;
  /** ms, as typed. */
  audioDelay: string;
  /** ms, as typed. */
  srtLatency: string;
  bitrateOverlay: boolean;
  server: "relay" | "own";
  relayServer: string;
  /** "" = use a stream ID instead. */
  relayAccount: string;
  address: string;
  port: string;
  /** Write-only: "" keeps the BELABOX's own. */
  streamId: string;
}

export type SettingsErrors = Partial<Record<keyof SettingsForm, string>>;

/** The form for what the BELABOX has now (with our unsent changes on top). */
export function settingsForm(details: BelaboxDetails): SettingsForm {
  const s = details.settings;
  const relay = s.relay_server != null || s.srtla_address == null;
  return {
    pipeline: s.pipeline ?? details.pipelines[0]?.id ?? "",
    audioSource: s.audio_source ?? details.audio_sources[0] ?? "",
    audioCodec: s.audio_codec ?? details.audio_codecs[0]?.id ?? "",
    audioDelay: String(s.audio_delay ?? 0),
    srtLatency: String(s.srt_latency ?? 2000),
    bitrateOverlay: s.bitrate_overlay ?? false,
    server: relay ? "relay" : "own",
    relayServer:
      s.relay_server ??
      details.relay_servers.find((server) => server.default)?.id ??
      details.relay_servers[0]?.id ??
      "",
    relayAccount: s.relay_account ?? "",
    address: s.srtla_address ?? "",
    port: s.srtla_port != null ? String(s.srtla_port) : "",
    streamId: "",
  };
}

export function selectedPipeline(details: BelaboxDetails, form: SettingsForm) {
  return details.pipelines.find((pipeline) => pipeline.id === form.pipeline);
}

/** Whole numbers only (BELABOX refuses fractions); `null` for anything else. */
export function wholeNumber(text: string): number | null {
  if (!/^-?\d+$/.test(text.trim())) return null;
  return Number(text.trim());
}

/** What's wrong with the form, field by field (mirrors the API, which has the last word). */
export function validateSettings(form: SettingsForm, details: BelaboxDetails): SettingsErrors {
  const errors: SettingsErrors = {};
  const pipeline = selectedPipeline(details, form);
  if (!pipeline) errors.pipeline = "Pick an encoder";

  if (pipeline?.audio_codec && !details.audio_codecs.some((codec) => codec.id === form.audioCodec))
    errors.audioCodec = "Pick an audio codec";
  if (pipeline?.audio_source) {
    const known =
      details.audio_sources.includes(form.audioSource) ||
      form.audioSource === details.settings.audio_source;
    if (!known) errors.audioSource = "Pick an audio source";
  }

  const delay = wholeNumber(form.audioDelay);
  if (delay == null || delay < AUDIO_DELAY_MIN || delay > AUDIO_DELAY_MAX)
    errors.audioDelay = `Between ${AUDIO_DELAY_MIN} and ${AUDIO_DELAY_MAX} ms`;
  const latency = wholeNumber(form.srtLatency);
  if (latency == null || latency < SRT_LATENCY_MIN || latency > SRT_LATENCY_MAX)
    errors.srtLatency = `Between ${SRT_LATENCY_MIN} and ${SRT_LATENCY_MAX} ms`;

  if (form.server === "relay") {
    if (!details.relay_servers.some((server) => server.id === form.relayServer))
      errors.relayServer = "Pick a relay server";
    if (form.relayAccount) {
      const account = details.relay_accounts.find((a) => a.id === form.relayAccount);
      if (!account) errors.relayAccount = "Pick a relay account";
    }
  } else {
    if (!form.address.trim() || /\s/.test(form.address.trim()))
      errors.address = "Enter the server's address";
    const port = wholeNumber(form.port);
    if (port == null || port < 1 || port > 65535) errors.port = "Between 1 and 65535";
  }

  const usesAccount = form.server === "relay" && form.relayAccount !== "";
  if (!usesAccount && !form.streamId && !details.settings.has_stream_id)
    errors.streamId = "Enter your stream ID";

  return errors;
}

/** The request for a valid form. */
export function settingsUpdate(form: SettingsForm, details: BelaboxDetails): BelaboxSettingsUpdate {
  const pipeline = selectedPipeline(details, form);
  const relay = form.server === "relay";
  const usesAccount = relay && form.relayAccount !== "";
  return {
    pipeline: form.pipeline,
    audio_source: pipeline?.audio_source ? form.audioSource : null,
    audio_codec: pipeline?.audio_codec ? form.audioCodec : null,
    audio_delay: Number(form.audioDelay),
    srt_latency: Number(form.srtLatency),
    bitrate_overlay: form.bitrateOverlay,
    relay_server: relay ? form.relayServer : null,
    relay_account: usesAccount ? form.relayAccount : null,
    srtla_address: relay ? null : form.address.trim(),
    srtla_port: relay ? null : Number(form.port),
    stream_id: usesAccount || !form.streamId ? null : form.streamId,
  };
}

export interface ModemForm {
  autoconfig: boolean;
  apn: string;
  username: string;
  /** Write-only: "" keeps the modem's own. */
  password: string;
  roaming: boolean;
  /** An operator ID, "" = automatic. */
  network: string;
  networkType: string;
}

export function modemForm(modem: BelaboxModemDetails): ModemForm {
  return {
    autoconfig: modem.config?.autoconfig ?? false,
    apn: modem.config?.apn ?? "",
    username: modem.config?.username ?? "",
    password: "",
    roaming: modem.config?.roaming ?? false,
    network: modem.config?.network ?? "",
    networkType: modem.network_type ?? modem.network_types[0] ?? "",
  };
}

export function modemErrors(
  form: ModemForm,
  modem: BelaboxModemDetails,
): Partial<Record<keyof ModemForm, string>> {
  const errors: Partial<Record<keyof ModemForm, string>> = {};
  if (!form.autoconfig && !form.apn.trim()) errors.apn = "Enter the APN";
  if (!modem.network_types.includes(form.networkType)) errors.networkType = "Pick a network type";
  return errors;
}

/** Whether the form differs from what the modem has (a typed password counts). */
export function modemDirty(form: ModemForm, modem: BelaboxModemDetails): boolean {
  const saved = modemForm(modem);
  return (Object.keys(saved) as (keyof ModemForm)[]).some((key) => saved[key] !== form[key]);
}

/** The operators to pick from: automatic, the saved one, and what the last scan found. */
export function operatorChoices(
  modem: BelaboxModemDetails,
): { id: string; label: string; disabled: boolean }[] {
  const choices = modem.networks.map((network) => ({
    id: network.id,
    label: network.availability ? `${network.name} (${network.availability})` : network.name,
    disabled: network.availability === "forbidden",
  }));
  const saved = modem.config?.network ?? "";
  if (saved && !choices.some((choice) => choice.id === saved))
    choices.unshift({ id: saved, label: `Operator ${saved}`, disabled: false });
  return choices;
}

/** Hotspot form validation (mirrors the API). `password` "" keeps the current one. */
export function hotspotErrors(
  name: string,
  password: string,
): { name?: string; password?: string } {
  const errors: { name?: string; password?: string } = {};
  if (!name.trim() || utf8Length(name) > NAME_MAX_BYTES)
    errors.name = `1 to ${NAME_MAX_BYTES} characters`;
  if (password && (password.length < WIFI_PASSWORD_MIN || password.length > HOTSPOT_PASSWORD_MAX))
    errors.password = `${WIFI_PASSWORD_MIN} to ${HOTSPOT_PASSWORD_MAX} characters`;
  return errors;
}

/** WiFi join validation (mirrors the API). An empty password is an open network. */
export function wifiJoinErrors(
  ssid: string,
  password: string,
): { ssid?: string; password?: string } {
  const errors: { ssid?: string; password?: string } = {};
  if (!ssid || utf8Length(ssid) > NAME_MAX_BYTES) errors.ssid = `1 to ${NAME_MAX_BYTES} characters`;
  if (password && (password.length < WIFI_PASSWORD_MIN || password.length > WIFI_PASSWORD_MAX))
    errors.password = `${WIFI_PASSWORD_MIN} to ${WIFI_PASSWORD_MAX} characters`;
  return errors;
}
