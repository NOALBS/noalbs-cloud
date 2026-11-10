import { t } from "@lingui/core/macro";
import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { Wordmark } from "@/components/wordmark";

export const TOP_TAB_CLASS =
  "flex shrink-0 items-center rounded-lg px-3 py-2 text-sm font-semibold text-muted-foreground hover:text-foreground data-[status=active]:bg-card data-[status=active]:text-foreground";

export function TopBar({
  pill,
  tabs,
  tabsLabel = t`Primary`,
  right,
}: {
  pill?: ReactNode;
  tabs?: ReactNode;
  tabsLabel?: string;
  right?: ReactNode;
}) {
  return (
    <header className="sticky top-0 z-20 hidden h-14 items-center gap-4 border-b border-divider bg-background/95 px-6 backdrop-blur md:flex">
      <div className="flex shrink-0 items-center gap-4">
        <Link to="/" aria-label={t`Home`} className="flex items-center">
          <Wordmark className="text-base" />
        </Link>
        {pill}
      </div>
      <nav
        aria-label={tabsLabel}
        className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto whitespace-nowrap [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
        {tabs}
      </nav>
      {right && <div className="flex shrink-0 items-center gap-2">{right}</div>}
    </header>
  );
}
