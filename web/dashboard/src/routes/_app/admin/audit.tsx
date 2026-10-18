import { useInfiniteQuery } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { adminAuditQuery } from "@/api/queries";
import type { AdminAuditEntry } from "@/api/types";
import { Page, Section } from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { describeAudit } from "@/lib/admin";
import { errorMessage } from "@/lib/errors";
import { currentLocale } from "@/lib/locale";
import { SUPPORT_LEVEL_LABELS } from "@/lib/support";
import { formatRelative } from "@/lib/time";
import { cn } from "@/lib/utils";

type Kind = "all" | "admin" | "support";

const KINDS: { value: Kind; label: string }[] = [
  { value: "all", label: "All" },
  { value: "admin", label: "Admin actions" },
  { value: "support", label: "Support in workspaces" },
];

export const Route = createFileRoute("/_app/admin/audit")({
  validateSearch: (search: Record<string, unknown>): { kind?: Kind } => ({
    kind: search.kind === "admin" || search.kind === "support" ? search.kind : undefined,
  }),
  component: AuditPage,
});

function AuditPage() {
  const kind = Route.useSearch().kind ?? "all";
  const audit = useInfiniteQuery(adminAuditQuery(kind));
  const entries = audit.data?.pages.flat() ?? [];

  return (
    <Page
      title="Audit log"
      description="What staff did: actions from Admin, and support work inside workspaces (which each workspace also sees). Kept for a year."
      className="max-w-5xl"
    >
      <div role="tablist" aria-label="Show" className="flex flex-wrap gap-1.5">
        {KINDS.map((option) => (
          <Link
            key={option.value}
            to="/admin/audit"
            search={option.value === "all" ? {} : { kind: option.value }}
            role="tab"
            aria-selected={kind === option.value}
            className={cn(
              "rounded-full border px-3.5 py-1.5 text-sm font-medium",
              kind === option.value
                ? "border-primary bg-primary-soft text-primary-soft-foreground"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {option.label}
          </Link>
        ))}
      </div>

      <Section title="Entries">
        {audit.isError && (
          <p className="px-5 pb-5 text-sm text-destructive">{errorMessage(audit.error)}</p>
        )}
        {audit.isPending && <Skeleton className="mx-5 mb-5 h-40" />}
        {audit.isSuccess && entries.length === 0 && (
          <p className="px-5 pb-5 text-sm text-muted-foreground">Nothing yet.</p>
        )}
        {entries.length > 0 && (
          <ul className="flex flex-col divide-y divide-divider border-t">
            {entries.map((entry) => (
              <AuditRow key={`${entry.kind}-${entry.id}`} entry={entry} />
            ))}
          </ul>
        )}
        {audit.hasNextPage && (
          <div className="px-5 py-3">
            <Button
              variant="ghost"
              onClick={() => audit.fetchNextPage()}
              disabled={audit.isFetchingNextPage}
            >
              Show older
            </Button>
          </div>
        )}
      </Section>
    </Page>
  );
}

function AuditRow({ entry }: { entry: AdminAuditEntry }) {
  const change = entry.kind === "admin" || entry.method !== "GET";

  return (
    <li className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1 px-5 py-2.5 text-sm">
      <span className="flex min-w-0 flex-col">
        <span className={change ? "font-semibold" : undefined}>
          {describeAudit(entry)}
          {entry.switcher_name && (
            <span className="font-normal text-muted-foreground"> · {entry.switcher_name}</span>
          )}
        </span>
        {entry.reason && <span className="text-muted-foreground">“{entry.reason}”</span>}
        <span className="text-xs text-muted-foreground">
          {entry.staff_name} · {formatRelative(entry.created_at)} ·{" "}
          {new Date(entry.created_at).toLocaleString(currentLocale())}
          {entry.workspace_id && entry.workspace_slug && (
            <>
              {" · "}
              <Link
                to="/admin/workspaces/$workspace"
                params={{ workspace: entry.workspace_id }}
                className="text-primary hover:underline"
              >
                workspace
              </Link>
            </>
          )}
          {entry.target_user_id && (
            <>
              {" · "}
              <Link
                to="/admin/users/$user"
                params={{ user: entry.target_user_id }}
                className="text-primary hover:underline"
              >
                user
              </Link>
            </>
          )}
        </span>
      </span>
      {entry.kind === "admin" ? (
        <Badge variant="secondary">Admin</Badge>
      ) : entry.break_glass ? (
        <Badge variant="outline" className="border-low/50 text-low">
          Emergency
        </Badge>
      ) : (
        <Badge variant="outline">
          {entry.grant_level ? SUPPORT_LEVEL_LABELS[entry.grant_level] : "No permission"}
        </Badge>
      )}
    </li>
  );
}
