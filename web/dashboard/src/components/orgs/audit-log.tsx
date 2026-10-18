import { useInfiniteQuery } from "@tanstack/react-query";
import { organizationAuditQuery } from "@/api/queries";
import type { Organization } from "@/api/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { currentLocale } from "@/lib/locale";
import { actorLabel, describeAudit } from "@/lib/org-audit";

/**
 * What the team, the customers and NOALBS changed in the organization: who, what
 * and when, newest first, kept a year.
 */
export function AuditLog({ organization }: { organization: Organization }) {
  const audit = useInfiniteQuery(organizationAuditQuery(organization.id));
  const entries = audit.data?.pages.flat() ?? [];

  return (
    <>
      {audit.isPending ? (
        <Skeleton className="h-64 rounded-2xl" />
      ) : entries.length === 0 ? (
        <p className="rounded-2xl border border-dashed px-4 py-8 text-center text-sm text-muted-foreground">
          Nothing yet. Changes your team makes show up here.
        </p>
      ) : (
        <ol className="flex flex-col overflow-hidden rounded-2xl border bg-card">
          {entries.map((entry) => {
            const { title, detail } = describeAudit(entry);
            const when = new Date(entry.created_at);
            return (
              <li
                key={entry.id}
                className="flex flex-col gap-0.5 border-b border-divider px-4 py-2.5 last:border-b-0"
              >
                <span className="flex items-baseline justify-between gap-3">
                  <span className="min-w-0 truncate text-sm font-semibold">{title}</span>
                  <time
                    dateTime={entry.created_at}
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
                <span className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
                  <span className="font-medium text-foreground">{actorLabel(entry)}</span>
                  {entry.actor === "staff" && <Badge variant="outline">NOALBS</Badge>}
                  {detail && <span>{detail}</span>}
                </span>
              </li>
            );
          })}
        </ol>
      )}
      {audit.hasNextPage && (
        <Button
          variant="outline"
          className="self-center"
          disabled={audit.isFetchingNextPage}
          onClick={() => void audit.fetchNextPage()}
        >
          {audit.isFetchingNextPage ? "Loading…" : "Load older"}
        </Button>
      )}
    </>
  );
}
