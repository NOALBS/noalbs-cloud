import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import {
  Activity,
  Bot,
  Building2,
  Gauge,
  Megaphone,
  Package,
  ScrollText,
  ShieldCheck,
  Users,
  Warehouse,
} from "lucide-react";
import type { ReactNode } from "react";
import { NoticeBanners } from "@/components/notice-banner";
import { TOP_TAB_CLASS, TopBar } from "@/components/top-bar";

const MOBILE_TAB =
  "flex min-h-10 shrink-0 items-center gap-1.5 rounded-full px-3.5 text-sm font-medium text-muted-foreground data-[status=active]:bg-card data-[status=active]:text-primary";

/** The admin section's pages, in menu order. */
export const adminPages = () =>
  [
    { label: t`Overview`, to: "/admin", icon: Gauge, exact: true },
    { label: t`Status`, to: "/admin/status", icon: Activity, exact: false },
    { label: t`Users`, to: "/admin/users", icon: Users, exact: false },
    { label: t`Workspaces`, to: "/admin/workspaces", icon: Warehouse, exact: false },
    { label: t`Organizations`, to: "/admin/organizations", icon: Building2, exact: false },
    { label: t`Products`, to: "/admin/products", icon: Package, exact: false },
    { label: t`Notices`, to: "/admin/notices", icon: Megaphone, exact: false },
    { label: t`Chat bots`, to: "/admin/chat-bots", icon: Bot, exact: false },
    { label: t`Audit log`, to: "/admin/audit", icon: ScrollText, exact: false },
  ] as const;

/** The staff-only admin section: its own navigation, apart from any workspace. */
export function AdminShell({ children }: { children: ReactNode }) {
  return (
    <div className="flex min-h-dvh flex-col">
      <TopBar
        pill={
          <span className="px-2 text-sm font-semibold">
            <Trans>Admin</Trans>
          </span>
        }
        tabsLabel={t`Admin`}
        tabs={adminPages().map((page) => (
          <Link
            key={page.to}
            to={page.to}
            activeOptions={{ exact: page.exact }}
            className={TOP_TAB_CLASS}
          >
            {page.label}
          </Link>
        ))}
        right={
          <Link to="/" className="text-sm font-semibold text-primary hover:underline">
            <Trans>Back to dashboard</Trans>
          </Link>
        }
      />
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-20 flex flex-col gap-2 border-b border-divider bg-background/95 px-4 pt-3 pb-2 backdrop-blur md:hidden">
          <div className="flex items-center justify-between">
            <span className="flex items-center gap-2 font-semibold">
              <ShieldCheck className="size-5 text-primary" />
              <Trans>Admin</Trans>
            </span>
            <Link to="/" className="flex min-h-11 items-center text-sm font-medium text-primary">
              <Trans>Back to dashboard</Trans>
            </Link>
          </div>
          <nav aria-label={t`Admin`} className="-mx-1 flex gap-1 overflow-x-auto">
            {adminPages().map((page) => (
              <Link
                key={page.to}
                to={page.to}
                activeOptions={{ exact: page.exact }}
                className={MOBILE_TAB}
              >
                <page.icon className="size-4" />
                {page.label}
              </Link>
            ))}
          </nav>
        </header>
        <NoticeBanners />
        <main className="flex-1">{children}</main>
      </div>
    </div>
  );
}
