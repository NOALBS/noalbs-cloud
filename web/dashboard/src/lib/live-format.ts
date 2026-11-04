import { t } from "@lingui/core/macro";
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
  const by = (who: string | null | undefined, fallback: string) => (who ? t`by ${who}` : fallback);

  switch (event.kind) {
    case "scene_switched": {
      const { from, to, reason, server, bitrate } = event.data;
      const title = from ? `${from} → ${to}` : t`Switched to ${to}`;
      if (reason === "auto") {
        const cause =
          bitrate != null && server
            ? t`${server} at ${formatKbps(bitrate)} kbps`
            : (server ?? t`the feed changed`);
        return { title, detail: t`Automatic · ${cause}` };
      }
      if (reason === "dashboard")
        return { title, detail: by(event.data.by, t`from the dashboard`) };
      return { title, detail: t`Changed in OBS` };
    }
    case "stream_started":
      return { title: t`Stream started`, detail: by(event.data.by, t`in OBS`) };
    case "stream_stopped":
      return { title: t`Stream stopped`, detail: by(event.data.by, t`in OBS`) };
    case "offline_timeout":
      return {
        title: t`Offline timeout`,
        detail: t`No feed for ${event.data.minutes} min on BRB, so the stream was stopped`,
      };
    case "ending_timeout":
      return {
        title: t`Ending timeout`,
        detail: t`Ending scene on for ${event.data.minutes} min, so the stream was stopped`,
      };
    case "recording_started":
      return { title: t`Recording started`, detail: by(event.data.by, t`in OBS`) };
    case "recording_stopped":
      return { title: t`Recording stopped`, detail: by(event.data.by, t`in OBS`) };
    case "recording_scene_paused":
      return {
        title: t`Recording paused`,
        detail: t`${event.data.scene} isn't one of the scenes to record on`,
      };
    case "recording_scene_resumed":
      return { title: t`Recording resumed`, detail: t`on ${event.data.scene}` };
    case "recording_scene_error":
      return { title: t`Recording problem`, detail: event.data.message };
    case "auto_switch_changed":
      return {
        title: event.data.enabled ? t`Auto switch on` : t`Auto switch off`,
        detail: by(event.data.by, t`from the dashboard`),
      };
    case "preview_off_air_changed":
      return {
        title: {
          nobody: t`Preview shown only while live`,
          owners: t`Preview shown to the owner off air`,
          admins: t`Preview shown to the owner and admins off air`,
          everyone: t`Preview shown to everyone off air`,
        }[event.data.off_air],
        detail: by(event.data.by, t`from the dashboard`),
      };
    case "connect_device_paired":
      return {
        title: t`Paired ${event.data.name}`,
        detail: by(event.data.by, t`from the dashboard`),
      };
    case "connect_device_revoked":
      return {
        title: t`Revoked ${event.data.name}`,
        detail: by(event.data.by, t`from the dashboard`),
      };
    case "connect_permissions_changed":
      return {
        title: t`Connect permissions changed`,
        detail: t`OBS control: ${event.data.obs_control} · Logs: ${event.data.logs} ${by(event.data.by, t`from the dashboard`)}`,
      };
    case "connect_watchdog_changed":
      return {
        title: t`Connect watchdog changed`,
        detail: t`Mode: ${event.data.mode} ${by(event.data.by, t`from the dashboard`)}`,
      };
    case "connect_obs_command": {
      const detail = event.data.by
        ? {
            start: t`${event.data.by} started OBS through Connect`,
            stop: t`${event.data.by} stopped OBS through Connect`,
            restart: t`${event.data.by} restarted OBS through Connect`,
            upload_log: t`${event.data.by} requested an OBS log through Connect`,
          }[event.data.action]
        : {
            start: t`Started OBS through Connect`,
            stop: t`Stopped OBS through Connect`,
            restart: t`Restarted OBS through Connect`,
            upload_log: t`Requested an OBS log through Connect`,
          }[event.data.action];
      return {
        title: t`OBS command`,
        detail,
      };
    }
    case "connect_log_uploaded": {
      const detail =
        event.data.kind === "obs_log"
          ? event.data.by
            ? t`${event.data.by} uploaded an OBS log through Connect`
            : t`Connect uploaded an OBS log`
          : event.data.by
            ? t`${event.data.by} uploaded a crash report through Connect`
            : t`Connect uploaded a crash report`;
      return {
        title: t`Connect log upload`,
        detail,
      };
    }
    case "feed_fixed":
      return {
        title: t`Fixed the feed`,
        detail: `${event.data.sources.join(", ")} ${by(event.data.by, t`from the dashboard`)}`,
      };
    case "marker_created":
      return {
        title: event.data.description ? t`Marker: ${event.data.description}` : t`Marker`,
        detail: t`At ${formatOffset(event.data.offset_seconds)} ${by(event.data.by, t`from the dashboard`)}`,
      };
    case "source_toggled":
      return {
        title: event.data.enabled ? t`Showed ${event.data.source}` : t`Hid ${event.data.source}`,
        detail: t`In ${event.data.scene} ${by(event.data.by, t`from the dashboard`)}`,
      };
    case "scene_created":
      return {
        title: t`Added the scene ${event.data.scene}`,
        detail: by(event.data.by, t`from the dashboard`),
      };
    case "scene_removed":
      return {
        title: t`Removed the scene ${event.data.scene}`,
        detail: by(event.data.by, t`from the dashboard`),
      };
    case "source_created":
      return {
        title: event.data.copied_from
          ? t`Copied ${event.data.copied_from} as ${event.data.source}`
          : t`Added ${event.data.source}`,
        detail: t`To ${event.data.scene} ${by(event.data.by, t`from the dashboard`)}`,
      };
    case "source_added":
      return {
        title: t`Added ${event.data.source} again`,
        detail: t`To ${event.data.scene} ${by(event.data.by, t`from the dashboard`)}`,
      };
    case "source_placed":
      return {
        title: t`Moved ${event.data.source}`,
        detail: t`In ${event.data.scene} ${by(event.data.by, t`from the dashboard`)}`,
      };
    case "source_removed":
      return {
        title: t`Removed ${event.data.source}`,
        detail: t`From ${event.data.scene} ${by(event.data.by, t`from the dashboard`)}`,
      };
    case "source_edited":
      return {
        title: t`Changed ${event.data.source}`,
        detail: event.data.renamed_from
          ? t`Renamed from ${event.data.renamed_from} ${by(event.data.by, t`from the dashboard`)}`
          : by(event.data.by, t`from the dashboard`),
      };
    case "input_muted":
      return {
        title: event.data.muted ? t`Muted ${event.data.input}` : t`Unmuted ${event.data.input}`,
        detail: by(event.data.by, t`from the dashboard`),
      };
    case "obs_connected":
      return { title: t`OBS connected`, detail: t`The browser source reached OBS` };
    case "obs_disconnected":
      return { title: t`OBS disconnected`, detail: t`The browser source lost OBS` };
    case "belabox_stream_started":
      return { title: t`BELABOX started`, detail: by(event.data.by, t`on the BELABOX`) };
    case "belabox_stream_stopped":
      return { title: t`BELABOX stopped`, detail: by(event.data.by, t`on the BELABOX`) };
    case "belabox_online":
      return { title: t`BELABOX online`, detail: t`It reached BELABOX Cloud` };
    case "belabox_offline":
      return { title: t`BELABOX offline`, detail: t`It left BELABOX Cloud` };
    case "belabox_bitrate_changed":
      return {
        title: t`BELABOX max bitrate ${formatKbps(event.data.max_br)} kbps`,
        detail: by(event.data.by, t`on the BELABOX`),
      };
    case "belabox_interface_toggled":
      return {
        title: event.data.enabled
          ? t`BELABOX ${event.data.name} on`
          : t`BELABOX ${event.data.name} off`,
        detail: by(event.data.by, t`on the BELABOX`),
      };
    case "belabox_configured":
      return {
        title: {
          wifi: t`BELABOX WiFi changed`,
          hotspot: t`BELABOX hotspot changed`,
          modem: t`BELABOX modem settings changed`,
          stream_settings: t`BELABOX stream settings changed`,
        }[event.data.what],
        detail: `${by(event.data.by, t`on the BELABOX`)}${event.data.what === "stream_settings" ? t`, used from its next start` : ""}`,
      };
    case "belabox_error":
      return { title: t`BELABOX problem`, detail: event.data.message };
    case "moblin_connected":
      return { title: t`Moblin connected`, detail: t`Its remote control reached NOALBS` };
    case "moblin_disconnected":
      return { title: t`Moblin disconnected`, detail: t`Its remote control went away` };
    case "moblin_wrong_password":
      return {
        title: t`Moblin password refused`,
        detail: t`A Moblin tried a wrong password. Check the one in its remote control settings.`,
      };
    case "moblin_stream_started":
      return { title: t`Moblin went live`, detail: by(event.data.by, t`on the phone`) };
    case "moblin_stream_stopped":
      return { title: t`Moblin stopped`, detail: by(event.data.by, t`on the phone`) };
    case "moblin_recording_started":
      return { title: t`Moblin recording`, detail: by(event.data.by, t`on the phone`) };
    case "moblin_recording_stopped":
      return { title: t`Moblin recording stopped`, detail: by(event.data.by, t`on the phone`) };
    case "moblin_mute_changed":
      return {
        title: event.data.muted ? t`Moblin muted` : t`Moblin unmuted`,
        detail: by(event.data.by, t`on the phone`),
      };
    case "moblin_scene_changed":
      return {
        title: t`Moblin scene ${event.data.scene}`,
        detail: by(event.data.by, t`on the phone`),
      };
    case "moblin_bitrate_changed":
      return {
        title: t`Moblin bitrate ${formatKbps(event.data.kbps)} kbps`,
        detail: by(event.data.by, t`on the phone`),
      };
    case "moblin_zoom_preset_changed":
      return {
        title: t`Moblin zoom ${event.data.name}`,
        detail: by(event.data.by, t`on the phone`),
      };
    case "moblin_torch_changed":
      return {
        title: event.data.on ? t`Moblin light on` : t`Moblin light off`,
        detail: by(event.data.by, t`on the phone`),
      };
    case "moblin_stealth_mode_changed":
      return {
        title: event.data.on ? t`Moblin stealth mode on` : t`Moblin stealth mode off`,
        detail: by(event.data.by, t`on the phone`),
      };
    case "moblin_connection_priorities_changed":
      return {
        title: event.data.enabled
          ? t`Moblin connection priorities on`
          : t`Moblin connection priorities off`,
        detail: by(event.data.by, t`on the phone`),
      };
    case "moblin_connection_changed":
      return {
        title: event.data.enabled
          ? t`Moblin ${event.data.name} on, priority ${event.data.priority}`
          : t`Moblin ${event.data.name} off`,
        detail: by(event.data.by, t`on the phone`),
      };
    case "moblin_error":
      return { title: t`Moblin problem`, detail: event.data.message };
  }
}
