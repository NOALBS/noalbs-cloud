import { t } from "@lingui/core/macro";
import { Link } from "@tanstack/react-router";
import { ChevronRight, MoreHorizontal } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { stateChip } from "./format";
import type { OperatorItem } from "./model";
import { OperatorName } from "./operator-name";
import { phoneDetail, phoneReading } from "./phone-format";
import { PhoneSheet } from "./phone-sheet";
import { useLongPress } from "./use-long-press";

export function PhoneRow({
  item,
  multi,
  now,
}: {
  item: OperatorItem;
  multi: boolean;
  now: number;
}) {
  const [open, setOpen] = useState(false);
  const press = useLongPress(() => setOpen(true));
  const chip = stateChip(item);
  const colour = chip.className
    .split(" ")
    .filter((token) => token.startsWith("text-"))
    .join(" ");
  return (
    <li
      {...press}
      className="relative grid select-none grid-cols-[8px_minmax(0,1fr)_auto_12px_28px] items-center gap-2 border-b border-divider px-3 py-3 last:border-b-0 [-webkit-touch-callout:none]"
    >
      <span aria-hidden="true" className={cn("size-1.5 rounded-full bg-current", colour)} />
      <div className="min-w-0">
        <Link
          to="/$workspace/s/$switcher/live"
          params={{ workspace: item.workspace_slug, switcher: item.switcher_id }}
          className="block truncate text-xs after:absolute after:inset-0 focus-visible:outline-none focus-visible:after:ring-2 focus-visible:after:ring-inset focus-visible:after:ring-ring"
        >
          <OperatorName
            workspace={item.workspace_name}
            switcher={item.switcher_name}
            multi={multi}
          />
        </Link>
        <p className="mt-0.5 truncate text-[11px] text-muted-foreground">
          {phoneDetail(item, now)}
        </p>
      </div>
      <span
        className={cn(
          "whitespace-nowrap font-display text-sm font-bold tabular-nums",
          item.attention || item.paused ? colour : "text-foreground",
          (!item.obs_connected || item.paused) && "text-[11px]",
        )}
      >
        {phoneReading(item, now)}
      </span>
      <ChevronRight className="size-3 text-muted-foreground" aria-hidden="true" />
      <Button
        variant="ghost"
        size="icon-sm"
        className="relative z-10 h-8 w-7"
        aria-label={t`Actions for ${item.workspace_name} / ${item.switcher_name}`}
        onClick={() => setOpen(true)}
      >
        <MoreHorizontal className="size-4" />
      </Button>
      {open && <PhoneSheet item={item} multi={multi} onClose={() => setOpen(false)} />}
    </li>
  );
}
