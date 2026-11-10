import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link, useLocation } from "@tanstack/react-router";
import { streamServersQuery, switchersQuery } from "@/api/queries";
import { DisplaySettings } from "@/components/display-settings";
import { Section } from "@/components/page";
import { ReadOnlyNotice } from "@/components/settings";
import { SettingsHub } from "@/components/settings-hub";
import {
  BrowserSourceLink,
  DeleteSwitcher,
  ObsConnection,
  PreviewPrivacy,
  SwitcherName,
} from "@/components/switcher-obs-content";
import { IndexedPage, SwitcherPage } from "@/components/switcher-page";
import { useMediaQuery } from "@/hooks/use-media-query";
import { useSwitcher } from "@/hooks/use-switcher";
import { setupProgress } from "@/lib/setup";
export const Route = createFileRoute("/_app/$workspace/s/$switcher/settings")({
  component: SettingsPage,
});
function SettingsPage() {
  const { workspace, switcher, canManage } = useSwitcher();
  const phone = useMediaQuery("(max-width: 767px)");
  const hash = useLocation({ select: (location) => location.hash });
  if (phone && !hash)
    return (
      <SwitcherPage title={t`Settings`}>
        {!canManage && <ReadOnlyNotice />}
        <SettingsHub />
      </SwitcherPage>
    );
  return (
    <IndexedPage
      title={t`Settings`}
      description={t`How ${switcher.name} connects to OBS, and how the dashboard looks for you.`}
      sections={[
        { id: "setup", title: t`Setup`, content: <SetupChecklist /> },
        { id: "obs", title: t`OBS`, content: <ObsConnection /> },
        {
          id: "browser-source",
          title: t`Browser source`,
          content: canManage ? (
            <BrowserSourceLink />
          ) : (
            <Section
              title={t`Browser source`}
              description={t`Only switcher managers can create a browser-source link.`}
            />
          ),
        },
        { id: "preview", title: t`Preview`, content: <PreviewPrivacy /> },
        { id: "display", title: t`Display`, content: <DisplaySettings /> },
        {
          id: "switcher",
          title: t`Switcher`,
          content: (
            <Section title={t`Switcher`} description={t`Rename or remove this switcher.`}>
              <SwitcherName />
              {canManage && <DeleteSwitcher />}
            </Section>
          ),
        },
      ]}
    >
      {phone && (
        <Link
          to="/$workspace/s/$switcher/settings"
          params={{ workspace: workspace.url_slug, switcher: switcher.id }}
          hash=""
          className="text-sm text-primary"
        >{t`Back to Settings`}</Link>
      )}
      {!canManage && <ReadOnlyNotice />}
    </IndexedPage>
  );
}
function SetupChecklist() {
  const { workspace, switcher } = useSwitcher();
  const servers = useQuery(streamServersQuery(workspace.id, switcher.id));
  const summary = useQuery(switchersQuery(workspace.id)).data?.find((s) => s.id === switcher.id);
  const params = { workspace: workspace.url_slug, switcher: switcher.id };
  const progress = servers.data
    ? setupProgress({
        connected: Boolean(summary?.status?.obs_connected) || switcher.last_seen_at != null,
        serverCount: servers.data.length,
        scenes: switcher.scenes,
      })
    : null;
  const steps = progress
    ? ([
        {
          label: t`Browser source added to OBS`,
          done: progress.connected,
          to: "/$workspace/s/$switcher/setup",
          hash: "",
        },
        {
          label: t`Stream server connected`,
          done: progress.hasServer,
          to: "/$workspace/s/$switcher/switching",
          hash: "servers",
        },
        {
          label: t`LIVE, LOW and BRB scenes picked`,
          done: progress.scenesSet,
          to: "/$workspace/s/$switcher/switching",
          hash: "scenes",
        },
      ] as const)
    : [];
  return (
    <Section
      title={t`Setup`}
      description={t`Connect OBS, a stream server and your scenes.`}
      actions={
        <Link to="/$workspace/s/$switcher/setup" params={params} className="text-sm text-primary">
          <Trans>Open setup guide</Trans>
        </Link>
      }
    >
      {servers.error && (
        <p className="px-5 pb-4 text-sm text-destructive">
          <Trans>Could not load setup progress.</Trans>
        </p>
      )}
      {!progress && !servers.error && (
        <p className="px-5 pb-4 text-sm text-muted-foreground">
          <Trans>Loading setup progress…</Trans>
        </p>
      )}
      {progress && (
        <div className="px-5 pb-4">
          <p className="mb-2 text-sm text-muted-foreground">{t`${progress.done} of ${progress.total} steps done`}</p>
          <div
            role="progressbar"
            aria-label={t`Setup progress`}
            aria-valuemin={0}
            aria-valuemax={progress.total}
            aria-valuenow={progress.done}
            className="h-1 rounded bg-muted"
          >
            <div
              className="h-full rounded bg-muted-foreground"
              style={{ width: `${(progress.done / progress.total) * 100}%` }}
            />
          </div>
        </div>
      )}
      {steps.map((step) => (
        <Link
          key={step.label}
          to={step.to}
          hash={step.hash}
          params={params}
          className="flex items-center justify-between gap-4 border-t border-divider px-5 py-4 text-sm"
        >
          <span>{step.label}</span>
          <span className="text-muted-foreground">{step.done ? t`Done` : t`Set up`}</span>
        </Link>
      ))}
      <Link
        to="/$workspace/s/$switcher/devices"
        params={params}
        className="block border-t border-divider px-5 py-4 text-sm text-primary"
      >
        <Trans>Set up optional devices</Trans>
      </Link>
    </Section>
  );
}
