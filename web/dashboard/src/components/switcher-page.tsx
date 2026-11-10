import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { Link, useLocation } from "@tanstack/react-router";
import { ChevronLeft } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { useSwitcher } from "@/hooks/use-switcher";
import { cn } from "@/lib/utils";

/** A switcher's pages, in sidebar order. Setup isn't listed: the sidebar shows it while
 * setup is unfinished, and Settings links to it. */
export const switcherPages = () =>
  [
    { label: t`Live`, to: "/$workspace/s/$switcher/live" },
    { label: t`History`, to: "/$workspace/s/$switcher/history" },
    { label: t`Switching`, to: "/$workspace/s/$switcher/switching" },
    { label: t`Chat`, to: "/$workspace/s/$switcher/chat" },
    { label: t`Devices`, to: "/$workspace/s/$switcher/devices" },
    { label: t`Settings`, to: "/$workspace/s/$switcher/settings" },
  ] as const;

/** The pages the mobile "Settings" tab covers. */
export const SETTINGS_SEGMENTS = [
  "switching",
  "devices",
  "settings",
  "triggers",
  "scenes",
  "servers",
  "chat",
  "obs",
  "setup",
];

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
          to="/$workspace/s/$switcher/settings"
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

export function IndexedPage({
  sections,
  children,
  lead,
  ...props
}: {
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  lead?: ReactNode;
  sections: { id: string; title: string; count?: number; content: ReactNode }[];
}) {
  const [active, setActive] = useState(sections[0]?.id);
  const hash = useLocation({ select: (location) => location.hash });
  const ids = sections.map((s) => s.id).join(",");
  useEffect(() => {
    const observer = new IntersectionObserver(
      (entries) => {
        const first = entries
          .filter((e) => e.isIntersecting)
          .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)[0];
        if (first) setActive(first.target.id);
      },
      { rootMargin: "-80px 0px -55% 0px" },
    );
    for (const id of ids.split(",")) {
      const el = document.getElementById(id);
      if (el) observer.observe(el);
    }
    return () => observer.disconnect();
  }, [ids]);
  useEffect(() => {
    if (!hash) return;
    const frame = requestAnimationFrame(() =>
      document.getElementById(hash.replace(/^#/, ""))?.scrollIntoView(),
    );
    return () => cancelAnimationFrame(frame);
  }, [hash]);
  return (
    <SwitcherPage {...props} className="max-w-7xl">
      {children}
      <div className="grid items-start gap-6 md:grid-cols-[200px_minmax(0,1fr)]">
        <nav aria-label={t`Page sections`} className="sticky top-24 hidden flex-col gap-1 md:flex">
          {sections.map((s) => (
            <a
              key={s.id}
              href={`#${s.id}`}
              onClick={() => setActive(s.id)}
              aria-current={active === s.id ? "location" : undefined}
              className={cn(
                "flex items-center justify-between gap-3 rounded-lg px-3 py-2 text-sm text-muted-foreground hover:bg-muted",
                active === s.id && "bg-muted font-semibold text-foreground",
              )}
            >
              <span>{s.title}</span>
              {s.count != null && (
                <span className="font-display text-xs text-muted-foreground">{s.count}</span>
              )}
            </a>
          ))}
        </nav>
        <div className="flex min-w-0 max-w-[940px] flex-col gap-4">
          {lead}
          {sections.map((s) => (
            <div key={s.id} id={s.id} className="scroll-mt-24">
              {s.content}
            </div>
          ))}
        </div>
      </div>
    </SwitcherPage>
  );
}
