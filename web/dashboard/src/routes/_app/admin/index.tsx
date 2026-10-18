import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { adminOverviewQuery } from "@/api/queries";
import { Stat } from "@/components/admin/list-controls";
import { Page, Section } from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { UserAvatar } from "@/components/user-avatar";
import { formatPrice } from "@/lib/billing";
import { formatRelative } from "@/lib/time";

export const Route = createFileRoute("/_app/admin/")({
  component: OverviewPage,
});

function OverviewPage() {
  const overview = useQuery(adminOverviewQuery);
  const data = overview.data;

  return (
    <Page
      title="Overview"
      description="NOALBS Cloud at a glance."
      className="max-w-6xl"
      actions={
        data &&
        (data.billing_enabled ? (
          <Badge variant={data.stripe_test_mode ? "secondary" : "default"}>
            Stripe {data.stripe_test_mode ? "test mode" : "live"}
          </Badge>
        ) : (
          <Badge variant="secondary">Billing off</Badge>
        ))
      }
    >
      {!data ? (
        <Skeleton className="h-40" />
      ) : (
        <>
          <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
            <Stat label="Users" value={data.users} hint={`${data.new_users_7d} new this week`} />
            <Stat label="Workspaces" value={data.workspaces} />
            <Stat
              label="Paying"
              value={data.paying_workspaces}
              hint={
                data.workspaces > 0
                  ? `${Math.round((data.paying_workspaces / data.workspaces) * 100)}% of workspaces`
                  : undefined
              }
            />
            <Stat
              label="MRR"
              value={
                data.mrr.length === 0
                  ? "—"
                  : data.mrr.map((mrr) => formatPrice(mrr.amount, mrr.currency)).join(" + ")
              }
              hint="Yearly prices ÷ 12"
            />
          </div>

          <div className="grid gap-4 md:grid-cols-2">
            <Section title="Workspaces per plan">
              <Bars rows={data.plans} total={data.workspaces} />
            </Section>
            <Section title="Add-ons sold" description="Units on live subscriptions.">
              <Bars
                rows={data.addons}
                total={Math.max(1, ...data.addons.map((addon) => addon.count))}
              />
            </Section>
          </div>

          <Section
            title="Newest users"
            actions={
              <Link to="/admin/users" className="text-sm font-medium text-primary hover:underline">
                All users
              </Link>
            }
          >
            <ul className="flex flex-col pb-2">
              {data.newest_users.map((user) => (
                <li key={user.id}>
                  <Link
                    to="/admin/users/$user"
                    params={{ user: user.id }}
                    className="flex items-center gap-3 px-5 py-2.5 hover:bg-surface-2"
                  >
                    <UserAvatar name={user.username} avatarUrl={user.avatar_url} />
                    <span className="min-w-0 flex-1 truncate text-sm font-medium">
                      {user.username ?? "No sign-in"}
                    </span>
                    <span className="text-xs text-muted-foreground">
                      {formatRelative(user.created_at)}
                    </span>
                  </Link>
                </li>
              ))}
            </ul>
          </Section>
        </>
      )}
    </Page>
  );
}

function Bars({
  rows,
  total,
}: {
  rows: { code: string; name: string; count: number }[];
  total: number;
}) {
  if (rows.length === 0) {
    return <p className="px-5 pb-5 text-sm text-muted-foreground">Nothing yet.</p>;
  }
  return (
    <ul className="flex flex-col gap-3 px-5 pb-5">
      {rows.map((row) => (
        <li key={row.code} className="flex flex-col gap-1">
          <span className="flex justify-between text-sm">
            <span className="font-medium">{row.name}</span>
            <span className="font-mono text-muted-foreground tabular">{row.count}</span>
          </span>
          <span className="h-1.5 overflow-hidden rounded-full bg-muted">
            <span
              className="block h-full rounded-full bg-primary"
              style={{ width: `${total > 0 ? (row.count / total) * 100 : 0}%` }}
            />
          </span>
        </li>
      ))}
    </ul>
  );
}
