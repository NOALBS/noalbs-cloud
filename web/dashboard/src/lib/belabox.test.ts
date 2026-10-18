import { describe, expect, it } from "vitest";
import type {
  BelaboxDetails,
  BelaboxInterface,
  BelaboxModemDetails,
  BelaboxResult,
  BelaboxStatus,
} from "@/api/types";
import {
  belaboxView,
  canDisable,
  describeResult,
  hotspotErrors,
  interfaceTitle,
  modemDirty,
  modemErrors,
  modemForm,
  modemLabel,
  operatorChoices,
  roundBitrate,
  type SettingsForm,
  settingsForm,
  settingsUpdate,
  signalBars,
  stepBitrate,
  unmatchedLabels,
  validateSettings,
  wifiBand,
  wifiJoinErrors,
  withLabel,
} from "./belabox";

const now = Date.parse("2026-09-27T12:00:00Z");

const status = (
  link: BelaboxStatus["link"],
  updatedAt = "2026-09-27T11:59:58Z",
): BelaboxStatus => ({
  updated_at: updatedAt,
  link,
  streaming: false,
  total_kbps: 0,
  interfaces: [],
  sensors: [],
  notifications: [],
});

const netif = (name: string, enabled: boolean): BelaboxInterface => ({ name, enabled, kbps: 0 });

describe("belaboxView", () => {
  it("follows the link, treating old statuses as gone", () => {
    expect(belaboxView(undefined, now)).toEqual({ kind: "loading" });
    expect(belaboxView(null, now)).toEqual({ kind: "idle" });
    expect(belaboxView(status("online", "2026-09-27T11:58:00Z"), now)).toEqual({ kind: "idle" });
    expect(belaboxView(status("invalid_key"), now)).toEqual({ kind: "invalid_key" });
    expect(belaboxView(status("encoder_offline"), now)).toEqual({ kind: "offline" });
    expect(belaboxView(status("online"), now).kind).toBe("online");
  });
});

describe("interface names", () => {
  const usb0 = { ...netif("usb0", true), key: "k1" };
  const wlan0 = { ...netif("wlan0", true), key: "k2" };

  it("uses the saved name of a device, else the interface name", () => {
    const labels = { k1: "Phone A" };
    expect(interfaceTitle(usb0, labels)).toBe("Phone A");
    expect(interfaceTitle(wlan0, labels)).toBe("wlan0");
    // A status from before keys existed.
    expect(interfaceTitle(netif("usb0", true), labels)).toBe("usb0");
  });

  it("lists saved names whose device is gone", () => {
    const labels = { k1: "Phone A", k9: "Phone B" };
    expect(unmatchedLabels([usb0, wlan0], labels)).toEqual([{ key: "k9", label: "Phone B" }]);
    expect(unmatchedLabels([], {})).toEqual([]);
  });

  it("sets, trims and removes a name", () => {
    expect(withLabel({}, "k1", "  Phone A ")).toEqual({ k1: "Phone A" });
    expect(withLabel({ k1: "Phone A", k2: "Phone B" }, "k1", "Phone C")).toEqual({
      k1: "Phone C",
      k2: "Phone B",
    });
    expect(withLabel({ k1: "Phone A", k2: "Phone B" }, "k1", "   ")).toEqual({ k2: "Phone B" });
  });
});

describe("canDisable", () => {
  it("keeps the last enabled interface on", () => {
    const interfaces = [netif("usb0", true), netif("wlan0", false)];
    expect(canDisable(interfaces, "usb0")).toBe(false);
    expect(canDisable([...interfaces, netif("usb1", true)], "usb0")).toBe(true);
  });
});

describe("stepBitrate", () => {
  it("moves on the 250 kbps grid within 300–12000", () => {
    expect(stepBitrate(6000, 1)).toBe(6250);
    expect(stepBitrate(6100, 1)).toBe(6250);
    expect(stepBitrate(6100, -1)).toBe(6000);
    expect(stepBitrate(500, -1)).toBe(300);
    expect(stepBitrate(300, -1)).toBe(300);
    expect(stepBitrate(12000, 1)).toBe(12000);
  });
});

describe("modemLabel", () => {
  it("lists what the modem reports", () => {
    expect(
      modemLabel({
        ...netif("usb0", true),
        modem: { network: "Vodafone", network_type: "4G", signal: 72, roaming: true },
      }),
    ).toBe("Vodafone · 4G · 72% · roaming");
    expect(modemLabel(netif("wlan0", true))).toBe("");
  });
});

describe("roundBitrate", () => {
  it("rounds to 250 kbps within BELABOX's range", () => {
    expect(roundBitrate(4100)).toBe(4000);
    expect(roundBitrate(4130)).toBe(4250);
    expect(roundBitrate(100)).toBe(300);
    expect(roundBitrate(20000)).toBe(12000);
    expect(roundBitrate(0)).toBeNull();
    expect(roundBitrate(Number.NaN)).toBeNull();
  });
});

// --- Settings dialog ---------------------------------------------------------

const details = (overrides: Partial<BelaboxDetails> = {}): BelaboxDetails => ({
  streaming: false,
  wifi: [],
  modems: [],
  audio_sources: ["HDMI", "USB audio"],
  audio_codecs: [
    { id: "opus", name: "Opus" },
    { id: "aac", name: "AAC" },
  ],
  pipelines: [
    { id: "full", name: "h265", audio_source: true, audio_codec: true },
    { id: "video", name: "h264 no audio", audio_source: false, audio_codec: false },
  ],
  relay_servers: [
    { id: "1", name: "EU", default: true },
    { id: "2", name: "US", default: false },
  ],
  relay_accounts: [
    { id: "7", name: "Mine", disabled: false },
    { id: "8", name: "Old", disabled: true },
  ],
  settings: {
    pipeline: "full",
    audio_source: "HDMI",
    audio_codec: "opus",
    audio_delay: 0,
    srt_latency: 2000,
    bitrate_overlay: false,
    relay_server: "1",
    relay_account: "7",
    has_stream_id: false,
  },
  settings_pending: false,
  result: null,
  ...overrides,
});

describe("signalBars and wifiBand", () => {
  it("rounds a signal to bars and names the band", () => {
    expect([null, 0, 10, 30, 60, 90].map(signalBars)).toEqual([0, 0, 1, 2, 3, 4]);
    expect([2412, 5180, 5955, null].map(wifiBand)).toEqual(["2.4 GHz", "5 GHz", "6 GHz", ""]);
  });
});

describe("settings form", () => {
  it("starts from what the BELABOX has", () => {
    const form = settingsForm(details());
    expect(form).toMatchObject({
      pipeline: "full",
      audioSource: "HDMI",
      audioCodec: "opus",
      audioDelay: "0",
      srtLatency: "2000",
      server: "relay",
      relayServer: "1",
      relayAccount: "7",
      streamId: "",
    });
    expect(validateSettings(form, details())).toEqual({});
  });

  it("starts on your own server when the BELABOX uses one", () => {
    const own = details({
      settings: {
        ...details().settings,
        relay_server: null,
        relay_account: null,
        srtla_address: "ingest.example.com",
        srtla_port: 5000,
        has_stream_id: true,
      },
    });
    const form = settingsForm(own);
    expect(form).toMatchObject({ server: "own", address: "ingest.example.com", port: "5000" });
    // The stream ID is the BELABOX's own until a new one is typed.
    expect(validateSettings(form, own)).toEqual({});
    expect(settingsUpdate(form, own)).toMatchObject({
      relay_server: null,
      srtla_address: "ingest.example.com",
      srtla_port: 5000,
      stream_id: null,
    });
  });

  it("checks the numbers, the server and the stream ID like the API", () => {
    const base = settingsForm(details());
    const errors = (change: Partial<SettingsForm>, d = details()) =>
      validateSettings({ ...base, ...change }, d);

    expect(errors({ audioDelay: "2001" }).audioDelay).toBeDefined();
    expect(errors({ audioDelay: "-2000" }).audioDelay).toBeUndefined();
    expect(errors({ audioDelay: "1.5" }).audioDelay).toBeDefined();
    expect(errors({ srtLatency: "99" }).srtLatency).toBeDefined();
    expect(errors({ srtLatency: "10000" }).srtLatency).toBeUndefined();
    expect(errors({ relayServer: "9" }).relayServer).toBeDefined();
    expect(errors({ audioCodec: "mp3" }).audioCodec).toBeDefined();
    expect(errors({ audioSource: "Mic" }).audioSource).toBeDefined();
    // The saved source may be unplugged right now.
    expect(errors({}, details({ audio_sources: ["USB audio"] })).audioSource).toBeUndefined();

    // Without an account a stream ID is needed, unless the BELABOX has one.
    expect(errors({ relayAccount: "" }).streamId).toBeDefined();
    expect(errors({ relayAccount: "", streamId: "live_1" }).streamId).toBeUndefined();
    expect(
      errors(
        { relayAccount: "" },
        details({ settings: { ...details().settings, has_stream_id: true } }),
      ).streamId,
    ).toBeUndefined();

    const own = {
      server: "own",
      address: " ",
      port: "0",
      relayAccount: "",
      streamId: "k",
    } as const;
    expect(errors(own).address).toBeDefined();
    expect(errors(own).port).toBeDefined();
    expect(errors({ ...own, address: "host", port: "65535" })).toEqual({});
  });

  it("ignores audio settings the pipeline doesn't take", () => {
    const form = { ...settingsForm(details()), pipeline: "video", audioCodec: "mp3" };
    expect(validateSettings(form, details())).toEqual({});
    expect(settingsUpdate(form, details())).toMatchObject({
      pipeline: "video",
      audio_source: null,
      audio_codec: null,
    });
  });

  it("builds the request for a relay and for your own server", () => {
    const base = settingsForm(details());
    expect(
      settingsUpdate({ ...base, audioDelay: "-150", bitrateOverlay: true }, details()),
    ).toEqual({
      pipeline: "full",
      audio_source: "HDMI",
      audio_codec: "opus",
      audio_delay: -150,
      srt_latency: 2000,
      bitrate_overlay: true,
      relay_server: "1",
      relay_account: "7",
      srtla_address: null,
      srtla_port: null,
      stream_id: null,
    });
    // An account replaces any typed stream ID.
    expect(settingsUpdate({ ...base, streamId: "ignored" }, details()).stream_id).toBeNull();
    expect(
      settingsUpdate({ ...base, relayAccount: "", streamId: "live_1" }, details()),
    ).toMatchObject({ relay_server: "1", relay_account: null, stream_id: "live_1" });
    expect(
      settingsUpdate(
        {
          ...base,
          server: "own",
          address: " host ",
          port: "5000",
          relayAccount: "",
          streamId: "k",
        },
        details(),
      ),
    ).toMatchObject({
      relay_server: null,
      srtla_address: "host",
      srtla_port: 5000,
      stream_id: "k",
    });
  });
});

describe("modem form", () => {
  const modem: BelaboxModemDetails = {
    id: "1",
    interface: "usb0",
    name: "EM7455",
    no_sim: false,
    network_types: ["3G", "4G"],
    network_type: "4G",
    config: {
      autoconfig: false,
      apn: "internet",
      username: "",
      has_password: true,
      roaming: false,
      network: "20408",
    },
    networks: [
      { id: "20404", name: "Vodafone", availability: "available" },
      { id: "20416", name: "T-Mobile", availability: "forbidden" },
    ],
    status: null,
  };

  it("is clean until something changes, and a typed password counts", () => {
    const form = modemForm(modem);
    expect(modemDirty(form, modem)).toBe(false);
    expect(modemDirty({ ...form, roaming: true }, modem)).toBe(true);
    expect(modemDirty({ ...form, password: "x" }, modem)).toBe(true);
  });

  it("needs an APN unless the BELABOX picks it", () => {
    const form = modemForm(modem);
    expect(modemErrors({ ...form, apn: " " }, modem).apn).toBeDefined();
    expect(modemErrors({ ...form, apn: "", autoconfig: true }, modem).apn).toBeUndefined();
    expect(modemErrors({ ...form, networkType: "5G" }, modem).networkType).toBeDefined();
  });

  it("offers the saved operator even when the scan didn't list it", () => {
    const choices = operatorChoices(modem);
    expect(choices.map((choice) => choice.id)).toEqual(["20408", "20404", "20416"]);
    expect(choices[2]).toMatchObject({ disabled: true, label: "T-Mobile (forbidden)" });
  });
});

describe("hotspot and WiFi forms", () => {
  it("checks names and passwords; an empty password keeps or means open", () => {
    expect(hotspotErrors("BELABOX", "")).toEqual({});
    expect(hotspotErrors("", "")).toHaveProperty("name");
    expect(hotspotErrors("x".repeat(33), "")).toHaveProperty("name");
    expect(hotspotErrors("é".repeat(17), "")).toHaveProperty("name");
    expect(hotspotErrors("ok", "short")).toHaveProperty("password");
    expect(hotspotErrors("ok", "x".repeat(64))).toEqual({});
    expect(wifiJoinErrors("Cafe", "")).toEqual({});
    expect(wifiJoinErrors("", "")).toHaveProperty("ssid");
    expect(wifiJoinErrors("Cafe", "x".repeat(64))).toHaveProperty("password");
  });
});

describe("describeResult", () => {
  const result = (kind: BelaboxResult["kind"], error: string | null): BelaboxResult => ({
    id: 1,
    kind,
    device: "0",
    error,
  });

  it("says what happened in words", () => {
    expect(describeResult(result("wifi_new", "auth"))).toEqual({
      ok: false,
      message: "Wrong WiFi password",
    });
    expect(describeResult(result("wifi_new", null)).ok).toBe(true);
    expect(describeResult(result("wifi_connect", "generic")).ok).toBe(false);
    expect(describeResult(result("hotspot_config", "password")).message).toContain("password");
    expect(describeResult(result("hotspot_config", "saving")).ok).toBe(false);
  });
});
