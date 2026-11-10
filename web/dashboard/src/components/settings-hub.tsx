import { plural, t } from "@lingui/core/macro";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import {
  Bot,
  ChevronRight,
  CircleHelp,
  Eye,
  Layers,
  Link2,
  Monitor,
  Palette,
  Radio,
  Server,
  Settings,
  SlidersHorizontal,
  Smartphone,
} from "lucide-react";
import { chatQuery, streamServersQuery } from "@/api/queries";
import { useDeviceSummary } from "@/hooks/use-device-summary";
import { useSwitcher } from "@/hooks/use-switcher";
import { previewOffAirOptions } from "@/lib/obs-preview";
import { setupProgress } from "@/lib/setup";
import { useTextSize } from "@/lib/text-size";
import { useTheme } from "@/lib/theme";

export function SettingsHub() {
  const { workspace, switcher } = useSwitcher();
  const servers = useQuery(streamServersQuery(workspace.id, switcher.id));
  const chat = useQuery(chatQuery(workspace.id, switcher.id));
  const devices = useDeviceSummary();
  const theme = useTheme();
  const text = useTextSize();
  const params = { workspace: workspace.url_slug, switcher: switcher.id };
  const progress = servers.data
    ? setupProgress({
        connected: Boolean(devices.status?.obs_connected) || switcher.last_seen_at != null,
        serverCount: servers.data.length,
        scenes: switcher.scenes,
      })
    : null;
  const sceneCount = [switcher.scenes.live, switcher.scenes.low, switcher.scenes.offline].filter(
    Boolean,
  ).length;
  const first = servers.data?.[0];
  const themeLabel = { system: t`System`, light: t`Light`, dark: t`Dark` }[theme.preference];
  const sizeLabel = { default: t`Default text`, large: t`Large text`, larger: t`Larger text` }[
    text.size
  ];
  const channels = chat.data?.channels;
  const botCount = channels?.filter((channel) => channel.bot != null).length ?? 0;
  const channelSummary = channels
    ? botCount > 0
      ? plural(channels.length, {
          one: "# channel · bot available",
          other: "# channels · bot available",
        })
      : plural(channels.length, {
          one: "# channel · no bot available",
          other: "# channels · no bot available",
        })
    : chat.isError
      ? t`Could not load channels`
      : t`Loading channels…`;
  const groups = [
    {
      title: t`Switching`,
      rows: [
        {
          icon: Server,
          label: t`Servers`,
          summary: servers.data
            ? first
              ? plural(servers.data.length, {
                  one: `# server · ${first.name} first`,
                  other: `# servers · ${first.name} first`,
                })
              : t`No servers yet`
            : servers.isError
              ? t`Could not load servers`
              : t`Loading servers…`,
          to: "/$workspace/s/$switcher/switching",
          hash: "servers",
        },
        {
          icon: Layers,
          label: t`Scenes`,
          summary: sceneCount === 3 ? t`LIVE, LOW and BRB set` : t`${sceneCount} of 3 set`,
          to: "/$workspace/s/$switcher/switching",
          hash: "scenes",
        },
        {
          icon: SlidersHorizontal,
          label: t`Rules`,
          summary:
            switcher.triggers.low != null
              ? t`LOW at ${switcher.triggers.low} kbps`
              : switcher.triggers.rtt != null
                ? t`LOW at ${switcher.triggers.rtt} ms RTT`
                : t`No LOW triggers set`,
          to: "/$workspace/s/$switcher/switching",
          hash: "rules",
        },
      ],
    },
    {
      title: t`Chat`,
      rows: [
        {
          icon: Bot,
          label: t`Chat`,
          summary: channelSummary,
          to: "/$workspace/s/$switcher/chat",
          hash: "",
        },
      ],
    },
    {
      title: t`Devices`,
      rows: [
        {
          icon: Radio,
          label: "BELABOX",
          summary: devices.belaboxText,
          to: "/$workspace/s/$switcher/devices",
          hash: "belabox",
        },
        {
          icon: Smartphone,
          label: "Moblin",
          summary: devices.moblinText,
          to: "/$workspace/s/$switcher/devices",
          hash: "moblin",
        },
        {
          icon: Monitor,
          label: t`Connect`,
          summary: devices.connectText,
          to: "/$workspace/s/$switcher/devices",
          hash: "connect",
        },
      ],
    },
    {
      title: t`This switcher`,
      rows: [
        {
          icon: CircleHelp,
          label: t`Setup`,
          summary: progress
            ? progress.complete
              ? t`Done`
              : t`${progress.done} of ${progress.total} steps done`
            : servers.isError
              ? t`Could not load setup progress.`
              : t`Loading setup progress…`,
          to: "/$workspace/s/$switcher/setup",
          hash: "",
        },
        {
          icon: Monitor,
          label: "OBS",
          summary:
            devices.status === undefined
              ? t`Loading status…`
              : devices.status?.obs_connected
                ? t`Connected · port ${switcher.obs.port}`
                : t`Not connected · port ${switcher.obs.port}`,
          to: "/$workspace/s/$switcher/settings",
          hash: "obs",
        },
        {
          icon: Link2,
          label: t`Browser source`,
          summary:
            devices.status === undefined
              ? t`Loading status…`
              : devices.status?.browser_source
                ? t`Connected`
                : t`Not connected`,
          to: "/$workspace/s/$switcher/settings",
          hash: "browser-source",
        },
        {
          icon: Eye,
          label: t`Preview`,
          summary:
            previewOffAirOptions(workspace.managed_by?.name ?? null).find(
              (option) => option.value === switcher.preview_off_air,
            )?.label ?? "",
          to: "/$workspace/s/$switcher/settings",
          hash: "preview",
        },
        {
          icon: Palette,
          label: t`Display`,
          summary: t`${themeLabel} · ${sizeLabel}`,
          to: "/$workspace/s/$switcher/settings",
          hash: "display",
        },
        {
          icon: Settings,
          label: t`Switcher`,
          summary: t`Rename or delete ${switcher.name}`,
          to: "/$workspace/s/$switcher/settings",
          hash: "switcher",
        },
      ],
    },
  ] as const;
  return (
    <nav aria-label={t`Settings sections`} className="flex flex-col gap-5 md:hidden">
      {groups.map((group) => (
        <section key={group.title} aria-label={group.title}>
          <h2 className="mb-2 px-1 text-xs font-semibold text-muted-foreground">{group.title}</h2>
          <div className="overflow-hidden rounded-2xl border bg-card shadow-tile">
            {group.rows.map((row) => (
              <Link
                key={row.label}
                to={row.to}
                hash={row.hash}
                params={params}
                className="flex min-h-16 items-center gap-3 border-t border-divider px-4 py-3 first:border-t-0"
              >
                <row.icon className="size-5 shrink-0 text-muted-foreground" aria-hidden="true" />
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="text-sm font-semibold">{row.label}</span>
                  <span className="break-words text-xs text-muted-foreground">{row.summary}</span>
                </span>
                <ChevronRight
                  className="size-4 shrink-0 text-muted-foreground"
                  aria-hidden="true"
                />
              </Link>
            ))}
          </div>
        </section>
      ))}
      <Link
        to="/$workspace/overview"
        params={{ workspace: workspace.url_slug }}
        className="flex min-h-11 items-center justify-between px-4 text-sm text-muted-foreground"
      >
        {t`Workspace · members, invites and plan`}
        <ChevronRight className="size-4" aria-hidden="true" />
      </Link>
    </nav>
  );
}
