import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Building2, Layers, Link2, Plus, Radio } from "lucide-react";
import type { ReactNode } from "react";
import { organizationsQuery, workspacesQuery } from "@/api/queries";
import type { Me } from "@/api/types";
import { standingLabel, TAB_CLASS, UserMenu } from "@/components/app-shell";
import { NoticeBanners } from "@/components/notice-banner";
import { TOP_TAB_CLASS, TopBar } from "@/components/top-bar";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { WhatsNewCard } from "@/components/whats-new-card";
import { WorkspaceTile } from "@/components/workspace-tile";

export function HomeShell({ me, children }: { me: Me; children: ReactNode }) {
  const workspaces = useQuery(workspacesQuery);
  const organizations = useQuery(organizationsQuery);
  const accountWorkspace = workspaces.data?.[0]?.url_slug ?? null;

  return (
    <div className="flex min-h-dvh flex-col">
      <TopBar
        pill={
          <DropdownMenu>
            <DropdownMenuTrigger
              className="flex h-9 items-center gap-2 rounded-xl border bg-card pl-1.5 pr-3 font-semibold outline-none focus-visible:ring-2 focus-visible:ring-ring data-[state=open]:bg-accent"
              aria-label={t`Switch workspace`}
            >
              <Layers className="size-4 text-muted-foreground" />
              <span className="text-sm">
                <Trans>All workspaces</Trans>
              </span>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-80">
              <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
                <Trans>Workspaces</Trans>
              </DropdownMenuLabel>
              {workspaces.data?.map((workspace) => (
                <DropdownMenuItem key={workspace.id} asChild>
                  <Link
                    to="/$workspace"
                    params={{ workspace: workspace.url_slug }}
                    className="flex min-h-10 items-center gap-2.5"
                  >
                    <WorkspaceTile seed={workspace.url_slug} name={workspace.name} size="sm" />
                    <span className="flex-1 truncate font-medium">{workspace.name}</span>
                    <span className="text-xs text-muted-foreground">
                      {standingLabel(workspace)}
                    </span>
                  </Link>
                </DropdownMenuItem>
              ))}
              {organizations.data && organizations.data.length > 0 && (
                <>
                  <DropdownMenuSeparator />
                  <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
                    <Trans>Organizations</Trans>
                  </DropdownMenuLabel>
                  {organizations.data.map((organization) => (
                    <DropdownMenuItem key={organization.id} asChild>
                      <Link
                        to="/orgs/$organization/clients"
                        params={{ organization: organization.url_slug }}
                      >
                        <Building2 />
                        <span className="flex-1 truncate">{organization.name}</span>
                      </Link>
                    </DropdownMenuItem>
                  ))}
                </>
              )}
              <DropdownMenuSeparator />
              <DropdownMenuItem asChild>
                <Link to="/welcome" search={{ mode: "create" }}>
                  <Plus />
                  <Trans>Create workspace</Trans>
                </Link>
              </DropdownMenuItem>
              <DropdownMenuItem asChild>
                <Link to="/welcome" search={{ mode: "join" }}>
                  <Link2 />
                  <Trans>Join with invite link</Trans>
                </Link>
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        }
        tabsLabel={t`All workspaces`}
        tabs={
          <>
            <Link to="/workspaces" activeOptions={{ exact: true }} className={TOP_TAB_CLASS}>
              <Trans>Workspaces</Trans>
            </Link>
            <Link to="/workspaces/live" className={TOP_TAB_CLASS}>
              <Trans>Live</Trans>
            </Link>
          </>
        }
        right={<UserMenu me={me} workspaceSlug={accountWorkspace} compact />}
      />
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-20 flex h-15 items-center justify-end gap-3 border-b border-divider bg-background/95 px-4 backdrop-blur md:hidden">
          <UserMenu me={me} workspaceSlug={accountWorkspace} compact />
        </header>
        <NoticeBanners />
        <WhatsNewCard me={me} />
        <main className="flex-1 pb-24 md:pb-0">{children}</main>
        <nav
          aria-label={t`Primary`}
          className="fixed inset-x-0 bottom-0 z-20 grid grid-cols-2 border-t border-divider bg-card/95 pb-[env(safe-area-inset-bottom)] backdrop-blur md:hidden"
        >
          <Link to="/workspaces" activeOptions={{ exact: true }} className={TAB_CLASS}>
            <Layers className="size-5.5" />
            <Trans>Workspaces</Trans>
          </Link>
          <Link to="/workspaces/live" className={TAB_CLASS}>
            <Radio className="size-5.5" />
            <Trans>Live</Trans>
          </Link>
        </nav>
      </div>
    </div>
  );
}
