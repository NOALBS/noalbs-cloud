import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { ChevronRight } from "lucide-react";
import { streamServersQuery, switchersQuery } from "@/api/queries";
import type { Scenes } from "@/api/types";
import { useSwitcher } from "@/hooks/use-switcher";
import { setupProgress } from "@/lib/setup";

const SCENE_LABELS: [keyof Scenes, string][] = [
  ["live", "LIVE"],
  ["low", "LOW"],
  ["offline", "BRB"],
  ["starting", "Starting"],
  ["ending", "Ending"],
  ["privacy", "Privacy"],
  ["refresh", "Refresh"],
];

/**
 * The phone's way into a switcher's settings: a list of sections, shown above
 * the switching settings (the desktop has the sidebar instead).
 */
export function SettingsHub() {
  const { workspace, switcher } = useSwitcher();
  const servers = useQuery(streamServersQuery(workspace.id, switcher.id));
  const summary = useQuery(switchersQuery(workspace.id)).data?.find(
    (item) => item.id === switcher.id,
  );
  // Setup isn't a tab on phones; this row leads to it until it's done.
  const progress = servers.data
    ? setupProgress({
        connected: Boolean(summary?.status?.obs_connected) || switcher.last_seen_at != null,
        serverCount: servers.data.length,
        scenes: switcher.scenes,
      })
    : null;
  const params = { workspace: workspace.url_slug, switcher: switcher.id };

  const scenes = SCENE_LABELS.filter(([key]) => switcher.scenes[key] != null).map(
    ([, label]) => label,
  );
  const enabled = servers.data?.filter((server) => server.enabled) ?? [];
  const serversSub = servers.data
    ? enabled.length === 0
      ? "None active"
      : `${enabled.length} active · ${enabled[0]?.name} first`
    : "…";

  const sections = [
    {
      label: "Scenes",
      sub: scenes.length ? scenes.join(" · ") : "None picked yet",
      to: "/$workspace/s/$switcher/scenes",
    },
    { label: "Stream servers", sub: serversSub, to: "/$workspace/s/$switcher/servers" },
    {
      label: "Chat & commands",
      sub: `Prefix ${switcher.chat.command_prefix}`,
      to: "/$workspace/s/$switcher/chat",
    },
    {
      label: "Switcher settings",
      sub: `OBS port ${switcher.obs.port} · browser-source link · BELABOX Cloud`,
      to: "/$workspace/s/$switcher/obs",
    },
  ] as const;

  return (
    <nav
      aria-label="Settings sections"
      className="flex flex-col overflow-hidden rounded-2xl border bg-card md:hidden"
    >
      {progress && !progress.complete && (
        <Link
          to="/$workspace/s/$switcher/setup"
          params={params}
          className="flex min-h-14 items-center gap-3 border-b border-divider bg-primary-soft px-3.5 py-2 text-primary-soft-foreground"
        >
          <SectionText
            label="Finish setup"
            sub={`${progress.done} of ${progress.total} steps done`}
          />
        </Link>
      )}
      {sections.map((section) => (
        <Link
          key={section.to}
          to={section.to}
          params={params}
          className="flex min-h-14 items-center gap-3 border-b border-divider px-3.5 py-2"
        >
          <SectionText label={section.label} sub={section.sub} />
        </Link>
      ))}
      <Link
        to="/$workspace/overview"
        params={{ workspace: workspace.url_slug }}
        className="flex min-h-14 items-center gap-3 px-3.5 py-2"
      >
        <SectionText label="Workspace" sub="Members, invites, plan" />
      </Link>
    </nav>
  );
}

function SectionText({ label, sub }: { label: string; sub: string }) {
  return (
    <>
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="text-[15px] font-semibold">{label}</span>
        <span className="truncate text-xs text-muted-foreground">{sub}</span>
      </span>
      <ChevronRight className="size-4.5 shrink-0 text-subtle-foreground" />
    </>
  );
}
