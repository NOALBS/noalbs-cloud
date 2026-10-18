import type { SwitcherEvent } from "@/api/types";
import { formatOffset } from "@/lib/history";
import { currentLocale } from "@/lib/locale";

/** "2:14:37" or "14:37", from a start time. */
export function formatUptime(startedAt: string, now = Date.now()): string {
  const seconds = Math.max(0, Math.floor((now - Date.parse(startedAt)) / 1000));
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

export function formatKbps(value: number): string {
  return value.toLocaleString(currentLocale());
}

/** A title and a detail line for the activity feed. */
export function describeEvent(event: SwitcherEvent): { title: string; detail: string } {
  const by = (who: string | null | undefined, fallback: string) => (who ? `by ${who}` : fallback);

  switch (event.kind) {
    case "scene_switched": {
      const { from, to, reason, server, bitrate } = event.data;
      const title = from ? `${from} → ${to}` : `Switched to ${to}`;
      if (reason === "auto") {
        const cause =
          bitrate != null && server
            ? `${server} at ${formatKbps(bitrate)} kbps`
            : (server ?? "the feed changed");
        return { title, detail: `Automatic · ${cause}` };
      }
      if (reason === "dashboard") return { title, detail: by(event.data.by, "from the dashboard") };
      return { title, detail: "Changed in OBS" };
    }
    case "stream_started":
      return { title: "Stream started", detail: by(event.data.by, "in OBS") };
    case "stream_stopped":
      return { title: "Stream stopped", detail: by(event.data.by, "in OBS") };
    case "offline_timeout":
      return {
        title: "Offline timeout",
        detail: `No feed for ${event.data.minutes} min on BRB, so the stream was stopped`,
      };
    case "ending_timeout":
      return {
        title: "Ending timeout",
        detail: `Ending scene on for ${event.data.minutes} min, so the stream was stopped`,
      };
    case "recording_started":
      return { title: "Recording started", detail: by(event.data.by, "in OBS") };
    case "recording_stopped":
      return { title: "Recording stopped", detail: by(event.data.by, "in OBS") };
    case "recording_scene_paused":
      return {
        title: "Recording paused",
        detail: `${event.data.scene} isn't one of the scenes to record on`,
      };
    case "recording_scene_resumed":
      return { title: "Recording resumed", detail: `on ${event.data.scene}` };
    case "recording_scene_error":
      return { title: "Recording problem", detail: event.data.message };
    case "auto_switch_changed":
      return {
        title: event.data.enabled ? "Auto switch on" : "Auto switch off",
        detail: by(event.data.by, "from the dashboard"),
      };
    case "feed_fixed":
      return {
        title: "Fixed the feed",
        detail: `${event.data.sources.join(", ")} ${by(event.data.by, "from the dashboard")}`,
      };
    case "marker_created":
      return {
        title: event.data.description ? `Marker: ${event.data.description}` : "Marker",
        detail: `At ${formatOffset(event.data.offset_seconds)} ${by(event.data.by, "from the dashboard")}`,
      };
    case "source_toggled":
      return {
        title: `${event.data.enabled ? "Showed" : "Hid"} ${event.data.source}`,
        detail: `In ${event.data.scene} ${by(event.data.by, "from the dashboard")}`,
      };
    case "input_muted":
      return {
        title: `${event.data.muted ? "Muted" : "Unmuted"} ${event.data.input}`,
        detail: by(event.data.by, "from the dashboard"),
      };
    case "obs_connected":
      return { title: "OBS connected", detail: "The browser source reached OBS" };
    case "obs_disconnected":
      return { title: "OBS disconnected", detail: "The browser source lost OBS" };
    case "belabox_stream_started":
      return { title: "BELABOX started", detail: by(event.data.by, "on the BELABOX") };
    case "belabox_stream_stopped":
      return { title: "BELABOX stopped", detail: by(event.data.by, "on the BELABOX") };
    case "belabox_online":
      return { title: "BELABOX online", detail: "It reached BELABOX Cloud" };
    case "belabox_offline":
      return { title: "BELABOX offline", detail: "It left BELABOX Cloud" };
    case "belabox_bitrate_changed":
      return {
        title: `BELABOX max bitrate ${formatKbps(event.data.max_br)} kbps`,
        detail: by(event.data.by, "on the BELABOX"),
      };
    case "belabox_interface_toggled":
      return {
        title: `BELABOX ${event.data.name} ${event.data.enabled ? "on" : "off"}`,
        detail: by(event.data.by, "on the BELABOX"),
      };
    case "belabox_configured":
      return {
        title: {
          wifi: "BELABOX WiFi changed",
          hotspot: "BELABOX hotspot changed",
          modem: "BELABOX modem settings changed",
          stream_settings: "BELABOX stream settings changed",
        }[event.data.what],
        detail: `${by(event.data.by, "on the BELABOX")}${event.data.what === "stream_settings" ? ", used from its next start" : ""}`,
      };
    case "belabox_error":
      return { title: "BELABOX problem", detail: event.data.message };
    case "moblin_connected":
      return { title: "Moblin connected", detail: "Its remote control reached NOALBS" };
    case "moblin_disconnected":
      return { title: "Moblin disconnected", detail: "Its remote control went away" };
    case "moblin_wrong_password":
      return {
        title: "Moblin password refused",
        detail: "A Moblin tried a wrong password. Check the one in its remote control settings.",
      };
    case "moblin_stream_started":
      return { title: "Moblin went live", detail: by(event.data.by, "on the phone") };
    case "moblin_stream_stopped":
      return { title: "Moblin stopped", detail: by(event.data.by, "on the phone") };
    case "moblin_recording_started":
      return { title: "Moblin recording", detail: by(event.data.by, "on the phone") };
    case "moblin_recording_stopped":
      return { title: "Moblin recording stopped", detail: by(event.data.by, "on the phone") };
    case "moblin_mute_changed":
      return {
        title: event.data.muted ? "Moblin muted" : "Moblin unmuted",
        detail: by(event.data.by, "on the phone"),
      };
    case "moblin_scene_changed":
      return {
        title: `Moblin scene ${event.data.scene}`,
        detail: by(event.data.by, "on the phone"),
      };
    case "moblin_bitrate_changed":
      return {
        title: `Moblin bitrate ${formatKbps(event.data.kbps)} kbps`,
        detail: by(event.data.by, "on the phone"),
      };
    case "moblin_zoom_preset_changed":
      return { title: `Moblin zoom ${event.data.name}`, detail: by(event.data.by, "on the phone") };
    case "moblin_torch_changed":
      return {
        title: event.data.on ? "Moblin light on" : "Moblin light off",
        detail: by(event.data.by, "on the phone"),
      };
    case "moblin_stealth_mode_changed":
      return {
        title: event.data.on ? "Moblin stealth mode on" : "Moblin stealth mode off",
        detail: by(event.data.by, "on the phone"),
      };
    case "moblin_connection_priorities_changed":
      return {
        title: `Moblin connection priorities ${event.data.enabled ? "on" : "off"}`,
        detail: by(event.data.by, "on the phone"),
      };
    case "moblin_connection_changed":
      return {
        title: event.data.enabled
          ? `Moblin ${event.data.name} on, priority ${event.data.priority}`
          : `Moblin ${event.data.name} off`,
        detail: by(event.data.by, "on the phone"),
      };
    case "moblin_error":
      return { title: "Moblin problem", detail: event.data.message };
  }
}
