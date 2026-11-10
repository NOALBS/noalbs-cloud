import { t } from "@lingui/core/macro";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { ChevronRight } from "lucide-react";
import { streamServersQuery, switchersQuery } from "@/api/queries";
import type { Scenes } from "@/api/types";
import { useSwitcher } from "@/hooks/use-switcher";
import { setupProgress } from "@/lib/setup";

const sceneLabels = (): [keyof Scenes, string][] => [
  ["live", "LIVE"],
  ["low", "LOW"],
  ["offline", "BRB"],
  ["starting", t`Starting`],
  ["ending", t`Ending`],
  ["privacy", t`Privacy`],
  ["refresh", t`Refresh`],
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

  const scenes = sceneLabels()
    .filter(([key]) => switcher.scenes[key] != null)
    .map(([, label]) => label);
  const enabled = servers.data?.filter((server) => server.enabled) ?? [];
  const serversSub = servers.data
    ? enabled.length === 0
      ? t`None active`
      : t`${enabled.length} active · ${enabled[0]?.name ?? ""} first`
    : "…";

  const sections = [
    {
      label: t`Scenes`,
      sub: scenes.length ? scenes.join(" · ") : t`None picked yet`,
      to: "/$workspace/s/$switcher/switching",
      hash: "scenes",
    },
    {
      label: t`Stream servers`,
      sub: serversSub,
      to: "/$workspace/s/$switcher/switching",
      hash: "servers",
    },
    {
      label: t`Chat & commands`,
      sub: t`Prefix ${switcher.chat.command_prefix}`,
      to: "/$workspace/s/$switcher/chat",
    },
    {
      label: t`Switcher settings`,
      sub: t`OBS port ${switcher.obs.port} · browser-source link · BELABOX Cloud`,
      to: "/$workspace/s/$switcher/settings",
      hash: "",
    },
  ] as const;

  return (
    <nav
      aria-label={t`Settings sections`}
      className="flex flex-col overflow-hidden rounded-2xl border bg-card md:hidden"
    >
      {progress && !progress.complete && (
        <Link
          to="/$workspace/s/$switcher/setup"
          params={params}
          className="flex min-h-14 items-center gap-3 border-b border-divider bg-primary-soft px-3.5 py-2 text-primary-soft-foreground"
        >
          <SectionText
            label={t`Finish setup`}
            sub={t`${progress.done} of ${progress.total} steps done`}
          />
        </Link>
      )}
      {sections.map((section) => (
        <Link
          key={section.label}
          to={section.to}
          hash={"hash" in section ? section.hash : undefined}
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
        <SectionText label={t`Workspace`} sub={t`Members, invites, plan`} />
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
