import { plural } from "@lingui/core/macro";
import { Link } from "@tanstack/react-router";
import { useMediaQuery } from "@/hooks/use-media-query";
import { cn } from "@/lib/utils";
import { useVisibleSwitchers, type VisibleSwitcherProps } from "./use-visible-switchers";

export function AttentionCount(props: VisibleSwitcherProps) {
  const desktop = useMediaQuery("(min-width: 48rem)");
  const { total, attention, groups } = useVisibleSwitchers(props, desktop);
  if (!desktop || total <= 1 || attention === 0) return null;
  return (
    <Link
      to="/workspaces/live"
      search={{ show: "attention" }}
      className={cn(
        "inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-lg border px-2.5 py-1.5 text-xs font-medium",
        groups.attention.some((item) => item.attention === "no_feed")
          ? "border-offline/30 bg-offline/5 text-offline"
          : "border-low/30 bg-low/5 text-low",
      )}
    >
      <span aria-hidden="true" className="size-1.5 rounded-full bg-current" />
      {plural(attention, { one: "# needs attention", other: "# need attention" })}
    </Link>
  );
}
