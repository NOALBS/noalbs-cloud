import { Link } from "@tanstack/react-router";
import type { OrganizationEvent } from "@/api/types";
import { describeEvent } from "@/lib/live-format";
import { currentLocale } from "@/lib/locale";

/** Events across the organization's clients: what happened, where, and when. */
export function ActivityList({ events }: { events: OrganizationEvent[] }) {
  return (
    <ol className="flex flex-col overflow-hidden rounded-2xl border bg-card shadow-tile">
      {events.map((item) => {
        const { title, detail } = describeEvent(item.event);
        const when = new Date(item.event.created_at);
        return (
          <li
            key={item.event.id}
            className="flex flex-col gap-1 border-t border-divider px-5 py-3 first:border-t-0"
          >
            <span className="flex flex-col gap-1 sm:flex-row-reverse sm:items-baseline sm:justify-end sm:gap-4">
              <span className="min-w-0 flex-1 text-sm font-semibold wrap-anywhere">{title}</span>
              <time
                dateTime={item.event.created_at}
                title={when.toLocaleString(currentLocale())}
                className="order-first shrink-0 font-display text-xs text-muted-foreground tabular-nums sm:order-none sm:w-24"
              >
                {when.toLocaleString(currentLocale(), {
                  month: "short",
                  day: "numeric",
                  hour: "2-digit",
                  minute: "2-digit",
                  hourCycle: "h23",
                })}
              </time>
            </span>
            <span className="flex flex-wrap items-baseline gap-x-2 text-xs text-muted-foreground wrap-anywhere sm:pl-28">
              <Link
                to="/$workspace/s/$switcher/live"
                params={{ workspace: item.workspace_slug, switcher: item.switcher_id }}
                className="font-medium text-foreground hover:underline"
              >
                {item.workspace_name} · {item.switcher_name}
              </Link>
              <span>{detail}</span>
            </span>
          </li>
        );
      })}
    </ol>
  );
}
