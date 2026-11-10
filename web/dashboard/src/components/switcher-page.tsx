import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { ChevronLeft } from "lucide-react";
import type { ReactNode } from "react";
import { useSwitcher } from "@/hooks/use-switcher";
import { cn } from "@/lib/utils";

/** A switcher's pages, in sidebar order. Setup isn't listed: the sidebar shows it while
 * setup is unfinished, and Settings links to it. */
export const switcherPages = () =>
  [
    { label: t`Live`, to: "/$workspace/s/$switcher/live" },
    { label: t`History`, to: "/$workspace/s/$switcher/history" },
    { label: t`Scenes`, to: "/$workspace/s/$switcher/scenes" },
    { label: t`Switching`, to: "/$workspace/s/$switcher/triggers" },
    { label: t`Stream servers`, to: "/$workspace/s/$switcher/servers" },
    { label: t`Chat & commands`, to: "/$workspace/s/$switcher/chat" },
    { label: t`Settings`, to: "/$workspace/s/$switcher/obs" },
  ] as const;

/** The pages the mobile "Settings" tab covers; its hub is the triggers page. */
export const SETTINGS_SEGMENTS = ["triggers", "scenes", "servers", "chat", "obs", "setup"];

/**
 * One of a switcher's pages: a title row with optional actions. Settings pages
 * get a way back to the settings hub on phones, where there is no sidebar.
 */
export function SwitcherPage({
  title,
  description,
  actions,
  backToSettings = false,
  back,
  className,
  children,
}: {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  backToSettings?: boolean;
  /** A link above the title, e.g. back to a list. */
  back?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  const { workspace, switcher } = useSwitcher();

  return (
    <div
      className={cn(
        "mx-auto flex w-full max-w-6xl flex-col gap-4 px-4 py-5 md:gap-5 md:px-8 md:py-6",
        className,
      )}
    >
      {backToSettings && (
        <Link
          to="/$workspace/s/$switcher/triggers"
          params={{ workspace: workspace.url_slug, switcher: switcher.id }}
          className="-mb-2 flex min-h-11 items-center gap-1 self-start text-sm font-medium text-primary md:hidden"
        >
          <ChevronLeft className="size-4" />
          <Trans>Settings</Trans>
        </Link>
      )}
      {back}
      <header className="flex flex-wrap items-center justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-1">
          <h1 className="font-display text-xl font-extrabold tracking-tight md:text-2xl">
            {title}
          </h1>
          {description && <p className="text-sm text-muted-foreground">{description}</p>}
        </div>
        {actions && <div className="flex flex-wrap items-center gap-2.5">{actions}</div>}
      </header>
      {children}
    </div>
  );
}
