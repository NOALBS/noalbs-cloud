import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { useCallback } from "react";
import { adminUsersQuery } from "@/api/queries";
import type { UserActivity, UserSort } from "@/api/types";
import { Pager, SearchBox } from "@/components/admin/list-controls";
import { Page } from "@/components/page";
import { Badge } from "@/components/ui/badge";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { UserAvatar } from "@/components/user-avatar";
import { formatDate, formatRelative } from "@/lib/time";

interface ListSearch {
  q?: string;
  page?: number;
  active?: UserActivity;
  sort?: UserSort;
}

const activity = (): { value: UserActivity | "any"; label: string }[] => [
  { value: "any", label: t`Active any time` },
  { value: "day", label: t`Active today` },
  { value: "week", label: t`Active this week` },
  { value: "month", label: t`Active this month` },
  { value: "inactive", label: t`Inactive 30+ days` },
];

const sorts = (): { value: UserSort; label: string }[] => [
  { value: "joined", label: t`Newest first` },
  { value: "active", label: t`Recently active` },
];

const oneOf = <T extends string>(values: { value: T | "any" }[], value: unknown) =>
  values.find((option) => option.value !== "any" && option.value === value)?.value as T | undefined;

export const Route = createFileRoute("/_app/admin/users/")({
  validateSearch: (search: Record<string, unknown>): ListSearch => ({
    q: typeof search.q === "string" && search.q ? search.q : undefined,
    page: Number(search.page) > 1 ? Number(search.page) : undefined,
    active: oneOf<UserActivity>(activity(), search.active),
    sort: oneOf<UserSort>(sorts(), search.sort === "joined" ? undefined : search.sort),
  }),
  component: UsersPage,
});

function UsersPage() {
  const { q = "", page = 1, active, sort } = Route.useSearch();
  const navigate = useNavigate({ from: Route.fullPath });
  const users = useQuery(adminUsersQuery({ q, page, active, sort }));
  const search = useCallback(
    (value: string) =>
      void navigate({
        search: (previous) => ({ ...previous, q: value || undefined, page: undefined }),
        replace: true,
      }),
    [navigate],
  );
  const filter = (patch: Partial<ListSearch>) =>
    void navigate({ search: (previous) => ({ ...previous, ...patch, page: undefined }) });

  return (
    <Page
      title={t`Users`}
      description={users.data ? t`${users.data.total} users` : t`Everyone who signed in.`}
      className="max-w-6xl"
    >
      <div className="flex flex-wrap items-center gap-2">
        <SearchBox value={q} onSearch={search} placeholder={t`Search by name or ID`} />
        <Select
          value={active ?? "any"}
          onValueChange={(value) => filter({ active: oneOf<UserActivity>(activity(), value) })}
        >
          <SelectTrigger className="w-44" aria-label={t`Last active`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {activity().map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select
          value={sort ?? "joined"}
          onValueChange={(value) =>
            filter({ sort: value === "joined" ? undefined : oneOf<UserSort>(sorts(), value) })
          }
        >
          <SelectTrigger className="w-40" aria-label={t`Sort`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {sorts().map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      {!users.data ? (
        <Skeleton className="h-64" />
      ) : users.data.items.length === 0 ? (
        <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
          <Trans>No users match.</Trans>
        </p>
      ) : (
        <ul className="flex flex-col overflow-hidden rounded-2xl border bg-card">
          <li className="hidden grid-cols-[1fr_7rem_9rem_8rem] gap-4 border-b border-divider px-4 py-2 text-xs tracking-wider text-muted-foreground uppercase md:grid">
            <span>
              <Trans>User</Trans>
            </span>
            <span>
              <Trans>Workspaces</Trans>
            </span>
            <span>
              <Trans>Last active</Trans>
            </span>
            <span>
              <Trans>Joined</Trans>
            </span>
          </li>
          {users.data.items.map((user) => (
            <li key={user.id} className="border-b border-divider last:border-b-0">
              <Link
                to="/admin/users/$user"
                params={{ user: user.id }}
                className="grid grid-cols-[1fr_auto] items-center gap-x-4 gap-y-1 px-4 py-3 hover:bg-surface-2 md:grid-cols-[1fr_7rem_9rem_8rem]"
              >
                <span className="flex min-w-0 items-center gap-3">
                  <UserAvatar name={user.username} avatarUrl={user.avatar_url} />
                  <span className="flex min-w-0 flex-col">
                    <span className="flex items-center gap-2 truncate text-sm font-medium">
                      {user.username ?? t`No sign-in`}
                      {user.staff && (
                        <Badge variant="secondary">
                          <Trans>Staff</Trans>
                        </Badge>
                      )}
                      {user.disabled_at && (
                        <Badge variant="destructive">
                          <Trans>Disabled</Trans>
                        </Badge>
                      )}
                    </span>
                    <span className="text-xs text-muted-foreground">
                      {user.provider?.toLowerCase() ?? "—"}
                    </span>
                  </span>
                </span>
                <span className="text-sm text-muted-foreground md:text-foreground">
                  <span className="md:hidden">
                    <Trans>Workspaces:</Trans>{" "}
                  </span>
                  {user.workspaces}
                </span>
                <span className="hidden text-sm text-muted-foreground md:block">
                  {user.last_active_at ? formatRelative(user.last_active_at) : t`Never`}
                </span>
                <span className="hidden text-sm text-muted-foreground md:block">
                  {formatDate(user.created_at)}
                </span>
              </Link>
            </li>
          ))}
        </ul>
      )}
      {users.data && (
        <Pager
          page={page}
          total={users.data.total}
          perPage={users.data.per_page}
          onPage={(next) => void navigate({ search: (previous) => ({ ...previous, page: next }) })}
        />
      )}
    </Page>
  );
}
