import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Building2, LayoutGrid, Link2, Plus, Radio } from "lucide-react";
import type { ReactNode } from "react";
import { organizationsQuery, workspacesQuery } from "@/api/queries";
import type { Me } from "@/api/types";
import { NAV_LINK, SECTION_LABEL, TAB_CLASS, UserMenu } from "@/components/app-shell";
import { NoticeBanners } from "@/components/notice-banner";
import { Wordmark } from "@/components/wordmark";

/**
 * The frame of the pages that span all your workspaces (the list and the live
 * overview): the same sidebar, phone app bar and tabs as a workspace, without being
 * inside one.
 */
export function HomeShell({ me, children }: { me: Me; children: ReactNode }) {
  const workspaces = useQuery(workspacesQuery);
  const organizations = useQuery(organizationsQuery);
  // The account page lives inside a workspace; any of your own will do.
  const accountWorkspace = workspaces.data?.[0]?.url_slug ?? null;

  return (
    <div className="flex min-h-dvh">
      <aside className="sticky top-0 hidden h-dvh w-66 shrink-0 flex-col gap-5 overflow-y-auto border-r border-divider bg-sunken px-3 py-3.5 md:flex">
        <Link to="/" aria-label="Home" className="flex h-12 shrink-0 items-center px-2.5">
          <Wordmark className="text-base" />
        </Link>

        <nav aria-label="All workspaces" className="flex flex-col gap-0.5">
          <span className={SECTION_LABEL}>ALL YOUR WORKSPACES</span>
          <Link to="/workspaces" activeOptions={{ exact: true }} className={NAV_LINK}>
            <LayoutGrid className="size-4" />
            <span className="flex-1">Workspaces</span>
            {workspaces.data && (
              <span className="font-mono text-[11px] tabular text-muted-foreground">
                {workspaces.data.length}
              </span>
            )}
          </Link>
          <Link to="/workspaces/live" className={NAV_LINK}>
            <Radio className="size-4" />
            <span className="flex-1">Live</span>
          </Link>
        </nav>

        {organizations.data && organizations.data.length > 0 && (
          <nav aria-label="Organizations" className="flex flex-col gap-0.5">
            <span className={SECTION_LABEL}>ORGANIZATIONS</span>
            {organizations.data.map((organization) => (
              <Link
                key={organization.id}
                to="/orgs/$organization/clients"
                params={{ organization: organization.url_slug }}
                className={NAV_LINK}
              >
                <Building2 className="size-4" />
                <span className="flex-1 truncate">{organization.name}</span>
              </Link>
            ))}
          </nav>
        )}

        <nav aria-label="Add" className="flex flex-col gap-0.5">
          <Link to="/welcome" search={{ mode: "create" }} className={NAV_LINK}>
            <Plus className="size-4" />
            Create workspace
          </Link>
          <Link to="/welcome" search={{ mode: "join" }} className={NAV_LINK}>
            <Link2 className="size-4" />
            Join with invite link
          </Link>
        </nav>

        <div className="mt-auto border-t border-divider pt-3">
          <UserMenu me={me} workspaceSlug={accountWorkspace} />
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-20 flex h-15 items-center justify-between gap-3 border-b border-divider bg-background/95 px-4 backdrop-blur md:hidden">
          <Link to="/" aria-label="Home" className="flex min-h-11 items-center">
            <Wordmark className="text-base" />
          </Link>
          <UserMenu me={me} workspaceSlug={accountWorkspace} compact />
        </header>

        <NoticeBanners />
        <main className="flex-1 pb-24 md:pb-0">{children}</main>

        <nav
          aria-label="Primary"
          className="fixed inset-x-0 bottom-0 z-20 grid grid-cols-2 border-t border-divider bg-sunken pb-[env(safe-area-inset-bottom)] md:hidden"
        >
          <Link to="/workspaces" activeOptions={{ exact: true }} className={TAB_CLASS}>
            <LayoutGrid className="size-5.5" />
            Workspaces
          </Link>
          <Link to="/workspaces/live" className={TAB_CLASS}>
            <Radio className="size-5.5" />
            Live
          </Link>
        </nav>
      </div>
    </div>
  );
}
