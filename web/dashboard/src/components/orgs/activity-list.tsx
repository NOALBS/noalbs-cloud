import { Link } from "@tanstack/react-router";
import type { OrganizationEvent } from "@/api/types";
import { describeEvent } from "@/lib/live-format";
import { currentLocale } from "@/lib/locale";

/** Events across the organization's clients: what happened, where, and when. */
export function ActivityList({ events }: { events: OrganizationEvent[] }) {
  return (
    <ol className="flex flex-col overflow-hidden rounded-2xl border bg-card">
      {events.map((item) => {
        const { title, detail } = describeEvent(item.event);
        const when = new Date(item.event.created_at);
        return (
          <li
            key={item.event.id}
            className="flex flex-col gap-0.5 border-b border-divider px-4 py-2.5 last:border-b-0"
          >
            <span className="flex items-baseline justify-between gap-3">
              <span className="min-w-0 truncate text-sm font-semibold">{title}</span>
              <time
                dateTime={item.event.created_at}
                title={when.toLocaleString(currentLocale())}
                className="shrink-0 font-mono text-[11px] text-muted-foreground tabular"
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
            <span className="flex flex-wrap items-baseline gap-x-2 text-xs text-muted-foreground">
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
