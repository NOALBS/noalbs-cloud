import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import {
  Activity,
  Bot,
  Building2,
  Check,
  ChevronsUpDown,
  LayoutList,
  Link2,
  Plus,
  Radio,
  Users,
  Warehouse,
} from "lucide-react";
import type { ReactNode } from "react";
import { organizationsQuery, workspacesQuery } from "@/api/queries";
import type { Me, Organization } from "@/api/types";
import {
  NAV_LINK,
  SECTION_LABEL,
  standingLabel,
  TAB_CLASS,
  UserMenu,
} from "@/components/app-shell";
import { NoticeBanners } from "@/components/notice-banner";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { WorkspaceTile } from "@/components/workspace-tile";
import { organizationRoleLabels } from "@/lib/organizations";
import { menuWorkspaces, recentWorkspaceIds } from "@/lib/recent-workspaces";
import { formatRelative } from "@/lib/time";
import { cn } from "@/lib/utils";

/**
 * The frame of an organization's pages, laid out like a workspace's: a sidebar
 * with the organization's menu (Clients, Team) and the account menu; an app bar
 * and tabs on phones. The clients are a page, not a list here: an organization
 * may have hundreds.
 */
export function OrgShell({
  organization,
  me,
  children,
}: {
  organization: Organization;
  me: Me;
  children: ReactNode;
}) {
  const params = { organization: organization.url_slug };
  const workspaces = useQuery(workspacesQuery);
  // The account page lives inside a workspace; any of your own will do.
  const accountWorkspace = workspaces.data?.[0]?.url_slug ?? null;
  const isAdmin = organization.role === "admin";

  return (
    <div className="flex min-h-dvh">
      <aside className="sticky top-0 hidden h-dvh w-66 shrink-0 flex-col gap-5 overflow-y-auto border-r border-divider bg-sunken px-3 py-3.5 md:flex">
        <OrganizationSwitcher organization={organization} />

        <nav aria-label={t`Organization`} className="flex flex-col gap-0.5">
          <span className={SECTION_LABEL}>
            <Trans>ORGANIZATION</Trans>
          </span>
          <Link to="/orgs/$organization/clients" params={params} className={NAV_LINK}>
            <Warehouse className="size-4" />
            <span className="flex-1">
              <Trans>Clients</Trans>
            </span>
            <span className="font-mono text-[11px] tabular text-muted-foreground">
              {organization.usage.clients}
            </span>
          </Link>
          <Link to="/orgs/$organization/live" params={params} className={NAV_LINK}>
            <Radio className="size-4" />
            <span className="flex-1">
              <Trans>Live</Trans>
            </span>
          </Link>
          {isAdmin && (
            <Link to="/orgs/$organization/activity" params={params} className={NAV_LINK}>
              <Activity className="size-4" />
              <span className="flex-1">
                <Trans>Activity</Trans>
              </span>
            </Link>
          )}
          <Link to="/orgs/$organization/chat" params={params} className={NAV_LINK}>
            <Bot className="size-4" />
            <span className="flex-1">
              <Trans>Chat bot</Trans>
            </span>
          </Link>
          <Link to="/orgs/$organization/team" params={params} className={NAV_LINK}>
            <Users className="size-4" />
            <span className="flex-1">
              <Trans>Team</Trans>
            </span>
            <span className="font-mono text-[11px] tabular text-muted-foreground">
              {organization.usage.team}
            </span>
          </Link>
        </nav>

        <div className="mt-auto border-t border-divider pt-3">
          <UserMenu me={me} workspaceSlug={accountWorkspace} />
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-20 flex h-15 items-center justify-between gap-3 border-b border-divider bg-background/95 px-4 backdrop-blur md:hidden">
          <OrganizationSwitcher organization={organization} compact />
          <UserMenu me={me} workspaceSlug={accountWorkspace} compact />
        </header>

        <NoticeBanners />
        {organization.suspension && (
          <div
            role="alert"
            className="border-b border-destructive/40 bg-destructive/10 px-4 py-2.5 text-sm md:px-8"
          >
            <Trans>
              <span className="font-semibold">NOALBS suspended this organization</span>{" "}
              {formatRelative(organization.suspension.since)}: {organization.suspension.reason}.
              Every client’s switchers are paused.
            </Trans>
          </div>
        )}
        <main className="flex-1 pb-24 md:pb-0">{children}</main>

        <nav
          aria-label={t`Primary`}
          className="fixed inset-x-0 bottom-0 z-20 grid grid-flow-col auto-cols-fr border-t border-divider bg-sunken pb-[env(safe-area-inset-bottom)] md:hidden"
        >
          <Link to="/orgs/$organization/clients" params={params} className={TAB_CLASS}>
            <Warehouse className="size-5.5" />
            <Trans>Clients</Trans>
          </Link>
          <Link to="/orgs/$organization/live" params={params} className={TAB_CLASS}>
            <Radio className="size-5.5" />
            <Trans>Live</Trans>
          </Link>
          {isAdmin && (
            <Link to="/orgs/$organization/activity" params={params} className={TAB_CLASS}>
              <Activity className="size-5.5" />
              <Trans>Activity</Trans>
            </Link>
          )}
          <Link to="/orgs/$organization/chat" params={params} className={TAB_CLASS}>
            <Bot className="size-5.5" />
            <Trans>Chat bot</Trans>
          </Link>
          <Link to="/orgs/$organization/team" params={params} className={TAB_CLASS}>
            <Users className="size-5.5" />
            <Trans>Team</Trans>
          </Link>
        </nav>
      </div>
    </div>
  );
}

/**
 * The organization at the top of the sidebar, opening a menu to the other
 * organizations and your own workspaces (the same places the workspace menu goes).
 */
function OrganizationSwitcher({
  organization,
  compact = false,
}: {
  organization: Organization;
  compact?: boolean;
}) {
  const organizations = useQuery(organizationsQuery);
  const workspaces = useQuery(workspacesQuery);
  const menu = menuWorkspaces(workspaces.data ?? [], null, recentWorkspaceIds());

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        className={cn(
          "flex shrink-0 items-center gap-2.5 rounded-lg text-left outline-none focus-visible:ring-2 focus-visible:ring-ring data-[state=open]:bg-surface-2",
          compact ? "min-h-11 min-w-0" : "h-12 px-2.5 hover:bg-surface-2",
        )}
        aria-label={t`Switch organization or workspace`}
      >
        <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-primary-soft text-primary-soft-foreground">
          <Building2 className="size-4" />
        </span>
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate font-semibold leading-tight">{organization.name}</span>
          {organization.role && (
            <span className="truncate text-xs text-muted-foreground">
              {organizationRoleLabels()[organization.role]}
            </span>
          )}
        </span>
        <ChevronsUpDown className="size-4 shrink-0 text-muted-foreground" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-75">
        <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
          <Trans>Organizations</Trans>
        </DropdownMenuLabel>
        {organizations.data?.map((candidate) => (
          <DropdownMenuItem key={candidate.id} asChild>
            <Link
              to="/orgs/$organization/clients"
              params={{ organization: candidate.url_slug }}
              className="flex min-h-10 items-center gap-2.5"
            >
              <Building2 className="size-4" />
              <span className="flex-1 truncate font-medium">{candidate.name}</span>
              {candidate.id === organization.id && <Check className="size-4 text-primary" />}
            </Link>
          </DropdownMenuItem>
        ))}
        {workspaces.data && workspaces.data.length > 0 && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
              {menu.hidden > 0 ? t`Recent workspaces` : t`Workspaces`}
            </DropdownMenuLabel>
            {menu.shown.map((workspace) => (
              <DropdownMenuItem key={workspace.id} asChild>
                <Link
                  to="/$workspace"
                  params={{ workspace: workspace.url_slug }}
                  className="flex min-h-10 items-center gap-2.5"
                >
                  <WorkspaceTile seed={workspace.url_slug} name={workspace.name} size="sm" />
                  <span className="flex-1 truncate font-medium">{workspace.name}</span>
                  <span className="text-xs text-muted-foreground">{standingLabel(workspace)}</span>
                </Link>
              </DropdownMenuItem>
            ))}
          </>
        )}
        <DropdownMenuSeparator />
        <DropdownMenuItem asChild>
          <Link to="/workspaces">
            <LayoutList />
            <Trans>All workspaces{menu.hidden > 0 && ` (${workspaces.data?.length ?? 0})`}</Trans>
          </Link>
        </DropdownMenuItem>
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
  );
}
