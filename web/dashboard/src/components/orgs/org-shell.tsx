import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { Link, useLocation } from "@tanstack/react-router";
import {
  Activity,
  Bot,
  Building2,
  Check,
  ChevronDown,
  LayoutList,
  Link2,
  Plus,
  Radio,
  Users,
  Warehouse,
} from "lucide-react";
import type { ReactNode } from "react";
import { organizationChatBotsQuery, organizationsQuery, workspacesQuery } from "@/api/queries";
import type { Me, Organization } from "@/api/types";
import {
  ChatAccessBanner,
  RECONNECT_LINK,
  standingLabel,
  TAB_CLASS,
  UserMenu,
} from "@/components/app-shell";
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
import { WorkspaceTile } from "@/components/workspace-tile";
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
    <div className="flex min-h-dvh flex-col">
      <TopBar
        pill={<OrganizationSwitcher organization={organization} />}
        tabsLabel={t`Organization`}
        tabs={
          <>
            <Link to="/orgs/$organization/clients" params={params} className={TOP_TAB_CLASS}>
              <Trans>Clients</Trans>
              <span className="ml-1 font-mono text-[11px] tabular text-muted-foreground">
                {organization.usage.clients}
              </span>
            </Link>
            <Link to="/orgs/$organization/live" params={params} className={TOP_TAB_CLASS}>
              <Trans>Live</Trans>
            </Link>
            {isAdmin && (
              <Link to="/orgs/$organization/activity" params={params} className={TOP_TAB_CLASS}>
                <Trans>Activity</Trans>
              </Link>
            )}
            <Link to="/orgs/$organization/chat" params={params} className={TOP_TAB_CLASS}>
              <Trans>Chat bot</Trans>
            </Link>
            <Link to="/orgs/$organization/team" params={params} className={TOP_TAB_CLASS}>
              <Trans>Team</Trans>
              <span className="ml-1 font-mono text-[11px] tabular text-muted-foreground">
                {organization.usage.team}
              </span>
            </Link>
          </>
        }
        right={<UserMenu me={me} workspaceSlug={accountWorkspace} compact />}
      />
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
        {isAdmin && <BotAccessBanners organization={organization} />}
        <main className="flex-1 pb-24 md:pb-0">{children}</main>

        <nav
          aria-label={t`Primary`}
          className="fixed inset-x-0 bottom-0 z-20 grid grid-flow-col auto-cols-fr border-t border-divider bg-card/95 pb-[env(safe-area-inset-bottom)] backdrop-blur md:hidden"
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
          "flex h-9 min-w-0 shrink-0 items-center gap-2 rounded-xl border bg-card pl-1.5 pr-3 text-left font-semibold outline-none focus-visible:ring-2 focus-visible:ring-ring data-[state=open]:bg-accent",
          compact && "max-w-52",
        )}
        aria-label={t`Switch organization or workspace`}
      >
        <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-primary-soft text-primary-soft-foreground">
          <Building2 className="size-4" />
        </span>
        <span className="truncate text-sm">{organization.name}</span>
        <ChevronDown className="size-4 shrink-0 text-muted-foreground" />
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

/** Tells admins the platform refused the organization's bot; its Chat bot page says so itself. */
function BotAccessBanners({ organization }: { organization: Organization }) {
  const bots = useQuery(organizationChatBotsQuery(organization.id));
  const pathname = useLocation({ select: (location) => location.pathname });
  if (pathname.endsWith("/chat")) return null;

  return bots.data
    ?.filter((bot) => bot.status === "revoked")
    .map((bot) => (
      <ChatAccessBanner
        key={bot.platform}
        link={
          <Link
            to="/orgs/$organization/chat"
            params={{ organization: organization.url_slug }}
            className={RECONNECT_LINK}
          >
            <Trans>Reconnect</Trans>
          </Link>
        }
      >
        {t`Your bot ${bot.display_name} lost its access; the shared bot answers for your clients until you reconnect it`}
      </ChatAccessBanner>
    ));
}
