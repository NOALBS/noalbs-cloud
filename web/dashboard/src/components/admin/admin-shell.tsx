import { Link } from "@tanstack/react-router";
import {
  Activity,
  Bot,
  Building2,
  ChevronLeft,
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
import { cn } from "@/lib/utils";

const NAV_LINK =
  "flex min-h-9.5 items-center gap-2.5 rounded-lg px-2.5 text-sm font-medium text-muted-foreground hover:bg-surface-2 hover:text-foreground data-[status=active]:bg-surface-2 data-[status=active]:font-semibold data-[status=active]:text-foreground";

const TAB =
  "flex min-h-10 shrink-0 items-center gap-1.5 rounded-full px-3.5 text-sm font-medium text-muted-foreground data-[status=active]:bg-surface-2 data-[status=active]:text-foreground";

/** The admin section's pages, in menu order. */
export const ADMIN_PAGES = [
  { label: "Overview", to: "/admin", icon: Gauge, exact: true },
  { label: "Status", to: "/admin/status", icon: Activity, exact: false },
  { label: "Users", to: "/admin/users", icon: Users, exact: false },
  { label: "Workspaces", to: "/admin/workspaces", icon: Warehouse, exact: false },
  { label: "Organizations", to: "/admin/organizations", icon: Building2, exact: false },
  { label: "Products", to: "/admin/products", icon: Package, exact: false },
  { label: "Notices", to: "/admin/notices", icon: Megaphone, exact: false },
  { label: "Chat bots", to: "/admin/chat-bots", icon: Bot, exact: false },
  { label: "Audit log", to: "/admin/audit", icon: ScrollText, exact: false },
] as const;

/** The staff-only admin section: its own navigation, apart from any workspace. */
export function AdminShell({ children }: { children: ReactNode }) {
  return (
    <div className="flex min-h-dvh">
      <aside className="sticky top-0 hidden h-dvh w-60 shrink-0 flex-col gap-5 border-r border-divider bg-sunken px-3 py-3.5 md:flex">
        <div className="flex items-center gap-2 px-2.5 pt-1 font-semibold">
          <ShieldCheck className="size-5 text-primary" />
          Admin
        </div>
        <nav aria-label="Admin" className="flex flex-col gap-0.5">
          {ADMIN_PAGES.map((page) => (
            <Link
              key={page.to}
              to={page.to}
              activeOptions={{ exact: page.exact }}
              className={NAV_LINK}
            >
              <page.icon className="size-4" />
              {page.label}
            </Link>
          ))}
        </nav>
        <Link to="/" className={cn(NAV_LINK, "mt-auto")}>
          <ChevronLeft className="size-4" />
          Back to NOALBS
        </Link>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-20 flex flex-col gap-2 border-b border-divider bg-background/95 px-4 pt-3 pb-2 backdrop-blur md:hidden">
          <div className="flex items-center justify-between">
            <span className="flex items-center gap-2 font-semibold">
              <ShieldCheck className="size-5 text-primary" />
              Admin
            </span>
            <Link to="/" className="flex min-h-11 items-center text-sm font-medium text-primary">
              Back to NOALBS
            </Link>
          </div>
          <nav aria-label="Admin" className="-mx-1 flex gap-1 overflow-x-auto">
            {ADMIN_PAGES.map((page) => (
              <Link
                key={page.to}
                to={page.to}
                activeOptions={{ exact: page.exact }}
                className={TAB}
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
