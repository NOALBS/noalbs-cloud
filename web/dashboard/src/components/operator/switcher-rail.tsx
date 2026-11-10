import { plural, t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { List, PanelLeftClose, PanelLeftOpen } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { useMediaQuery } from "@/hooks/use-media-query";
import { useNow } from "@/hooks/use-now";
import { cn } from "@/lib/utils";
import { stateChip } from "./format";
import type { OperatorItem } from "./model";
import { phoneDetail, phoneReading } from "./phone-format";
import { useVisibleSwitchers, type VisibleSwitcherProps } from "./use-visible-switchers";

export const RAIL_KEY = "noalbs-operator-rail-collapsed";
function savedCollapsed() {
  try {
    return localStorage.getItem(RAIL_KEY) === "true";
  } catch {
    return false;
  }
}

export function SwitcherRail({
  currentId,
  children,
  ...props
}: VisibleSwitcherProps & { currentId?: string; children: ReactNode }) {
  const desktop = useMediaQuery("(min-width: 64rem)");
  const data = useVisibleSwitchers(props, desktop && Boolean(currentId));
  const [collapsed, setCollapsed] = useState(savedCollapsed);
  const now = useNow();
  if (!desktop || !currentId || data.total <= 1) return <>{children}</>;
  const toggle = () => {
    const next = !collapsed;
    setCollapsed(next);
    try {
      localStorage.setItem(RAIL_KEY, String(next));
    } catch {
      /* Storage may be unavailable. */
    }
  };
  const labels = { attention: t`Needs attention`, streaming: t`Streaming`, idle: t`Not streaming` };
  return (
    <div className="flex min-w-0 flex-1 items-start [&>main]:min-w-0">
      <aside
        aria-label={t`Switcher rail`}
        data-collapsed={collapsed}
        className={cn(
          "sticky top-14 flex h-[calc(100dvh-3.5rem)] shrink-0 flex-col border-r border-divider bg-background",
          collapsed ? "w-12" : "w-[212px]",
        )}
      >
        <header
          className={cn(
            "flex h-12 shrink-0 items-center gap-2 px-2",
            collapsed ? "justify-center" : "justify-between pl-4",
          )}
        >
          {!collapsed && (
            <h2 className="text-xs font-semibold">
              <Trans>Switchers</Trans>
            </h2>
          )}
          <Button
            size="icon-sm"
            variant="ghost"
            onClick={toggle}
            aria-expanded={!collapsed}
            aria-controls="operator-rail-items"
            aria-label={collapsed ? t`Expand switcher rail` : t`Collapse switcher rail`}
          >
            {collapsed ? (
              <PanelLeftOpen className="size-4" />
            ) : (
              <PanelLeftClose className="size-4" />
            )}
          </Button>
        </header>
        <nav
          id="operator-rail-items"
          aria-label={t`Switchers`}
          className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-1.5 pb-3"
        >
          <TooltipProvider>
            {(["attention", "streaming", "idle"] as const).map(
              (group) =>
                data.groups[group].length > 0 && (
                  <section key={group} aria-label={labels[group]} className="mb-3">
                    {!collapsed && (
                      <h3 className="px-2 py-2 text-[10px] font-medium uppercase tracking-wide text-muted-foreground">
                        {labels[group]}
                      </h3>
                    )}
                    <ul className="flex flex-col gap-0.5">
                      {data.groups[group].map((item) => (
                        <RailRow
                          key={item.switcher_id}
                          item={item}
                          current={item.switcher_id === currentId}
                          collapsed={collapsed}
                          now={now}
                        />
                      ))}
                    </ul>
                  </section>
                ),
            )}
          </TooltipProvider>
        </nav>
        <footer className="shrink-0 border-t border-divider p-2">
          <Link
            to="/workspaces/live"
            aria-label={t`All switchers`}
            title={collapsed ? t`All switchers` : undefined}
            className={cn(
              "flex items-center gap-2 rounded-lg py-2 text-xs hover:bg-surface-2",
              collapsed ? "justify-center" : "px-1",
            )}
          >
            <List className="size-3.5 shrink-0" />
            {!collapsed && (
              <>
                <span>
                  <Trans>All switchers</Trans>
                </span>
                <span className="ml-auto text-[10px] text-muted-foreground">
                  {plural(data.offline, { one: "# offline", other: "# offline" })}
                </span>
              </>
            )}
          </Link>
        </footer>
      </aside>
      {children}
    </div>
  );
}

function RailRow({
  item,
  current,
  collapsed,
  now,
}: {
  item: OperatorItem;
  current: boolean;
  collapsed: boolean;
  now: number;
}) {
  const colour = stateChip(item)
    .className.split(" ")
    .filter((token) => token.startsWith("text-"))
    .join(" ");
  const label = t`${item.workspace_name} / ${item.switcher_name} · ${stateChip(item).label} · ${phoneDetail(item, now)}`;
  const link = (
    <Link
      to="/$workspace/s/$switcher/live"
      params={{ workspace: item.workspace_slug, switcher: item.switcher_id }}
      aria-current={current ? "page" : undefined}
      aria-label={collapsed ? label : undefined}
      className={cn(
        "flex min-w-0 items-center gap-2 rounded-lg border-l-2 border-l-transparent py-2 text-xs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring hover:bg-surface-2",
        collapsed ? "justify-center px-1" : "px-2",
        current && "border-l-primary bg-primary-soft",
      )}
    >
      <span
        aria-hidden="true"
        className={cn("size-1.5 shrink-0 rounded-full bg-current", colour)}
      />
      {!collapsed && (
        <>
          <span className="min-w-0 flex-1">
            <span className="block truncate font-semibold">{item.switcher_name}</span>
            <span className="mt-0.5 block truncate text-[10px] text-muted-foreground">
              {item.scene ? t`${item.workspace_name} · ${item.scene}` : item.workspace_name}
            </span>
          </span>
          <span
            className={cn(
              "shrink-0 whitespace-nowrap font-display text-[11px] font-semibold tabular-nums",
              item.attention || item.paused ? colour : "text-foreground",
            )}
          >
            {phoneReading(item, now)}
          </span>
        </>
      )}
    </Link>
  );
  return (
    <li>
      {collapsed ? (
        <Tooltip>
          <TooltipTrigger asChild>{link}</TooltipTrigger>
          <TooltipContent side="right">{label}</TooltipContent>
        </Tooltip>
      ) : (
        link
      )}
    </li>
  );
}
