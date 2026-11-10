import { plural, t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useLocation, useNavigate, useParams } from "@tanstack/react-router";
import {
  Activity,
  AlertTriangle,
  Building2,
  Check,
  ChevronDown,
  ChevronLeft,
  Headset,
  History,
  House,
  LayoutGrid,
  LayoutList,
  Link2,
  LogOut,
  Monitor,
  Moon,
  Newspaper,
  Plus,
  Settings,
  ShieldCheck,
  Sun,
  UserCog,
  Users,
} from "lucide-react";
import { type ReactNode, useState } from "react";
import { api } from "@/api/client";
import {
  chatBotsQuery,
  organizationsQuery,
  supportAccessQuery,
  switcherQuery,
  switchersQuery,
  workspacePlanQuery,
} from "@/api/queries";
import type { Me, Organization, SwitcherSummary, Workspace } from "@/api/types";
import { NoticeBanners } from "@/components/notice-banner";
import { Badge } from "@/components/ui/badge";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { hasUnseenPosts, WhatsNewCard } from "@/components/whats-new-card";
import { missingCardReminder } from "@/lib/billing";
import { botDetail, revokedOwnBots, speakingBot, switchersToReconnect } from "@/lib/chat";
import { canControlConnect, canManageConnectPrograms } from "@/lib/connect";
import { lastSwitcherId } from "@/lib/last-switcher";
import { currentLocale } from "@/lib/locale";
import { type Actor, actorOf, can, isSupport, roleLabels } from "@/lib/permissions";
import { menuWorkspaces, recentWorkspaceIds } from "@/lib/recent-workspaces";
import { setupProgress } from "@/lib/setup";
import { supportLevelLabels } from "@/lib/support";
import { type TextSize, useTextSize } from "@/lib/text-size";
import { type ThemePreference, useTheme } from "@/lib/theme";
import { cn } from "@/lib/utils";
import { WHATS_NEW_PATH } from "@/lib/whats-new";
import { ConnectDialog } from "./connect-controls";
import { CreateSwitcherDialog } from "./create-switcher-dialog";
import { LanguageMenu } from "./language-menu";
import { ObsStatsPopover } from "./live/obs-stats-popover";
import { SETTINGS_SEGMENTS, switcherPages } from "./switcher-page";
import { StatusDot, switcherStatus } from "./switcher-status";
import { TOP_TAB_CLASS, TopBar } from "./top-bar";
import { providerLabel, UserAvatar } from "./user-avatar";
import { WorkspaceTile } from "./workspace-tile";

export const TAB_CLASS =
  "flex h-17 flex-col items-center justify-center gap-1 text-xs font-medium text-muted-foreground data-[status=active]:font-semibold data-[status=active]:text-primary";

/** What to call the viewer's standing in a workspace of the menu. */
export function standingLabel(workspace: Workspace): string {
  if (workspace.role) return roleLabels()[workspace.role];
  if (workspace.organization_role) return t`Organization`;
  return t`Staff`;
}

function profileOf(me: Me) {
  const connection = me.connections[0];
  return {
    name: connection?.username ?? t`Account`,
    avatarUrl: connection?.avatar_url ?? null,
    provider: providerLabel(connection?.provider),
  };
}

export function AppShell({
  workspace,
  workspaces,
  me,
  children,
}: {
  workspace: Workspace;
  workspaces: Workspace[];
  me: Me;
  children: ReactNode;
}) {
  const [sheetOpen, setSheetOpen] = useState(false);
  const params = { workspace: workspace.url_slug };
  const routeSwitcher = useParams({ strict: false }).switcher;
  const pathname = useLocation({ select: (location) => location.pathname });
  const switchers = useQuery(switchersQuery(workspace.id));
  const organizations = useQuery(organizationsQuery);
  const actor = actorOf(workspace, me);
  // With many workspaces the sheet lists the recent ones; the rest are on All workspaces.
  const menu = menuWorkspaces(workspaces, workspace.id, recentWorkspaceIds());
  const canAddSwitcher = can(actor, "switcher.manage");

  // The switcher the navigation is about: the open one, else the one used last
  // in this browser, else the first.
  const remembered = lastSwitcherId(workspace.id);
  const current =
    switchers.data?.find((switcher) => switcher.id === routeSwitcher) ??
    switchers.data?.find((switcher) => switcher.id === remembered) ??
    switchers.data?.[0];
  const inSwitcher = Boolean(routeSwitcher && current?.id === routeSwitcher);
  const segment = pathname.split("/").at(-1) ?? "";
  const settingsActive =
    (inSwitcher && SETTINGS_SEGMENTS.includes(segment)) || pathname.includes("/settings/");

  return (
    <div className="flex min-h-dvh flex-col">
      <TopBar
        pill={
          <WorkspaceMenu
            workspace={workspace}
            workspaces={workspaces}
            organizations={organizations.data ?? []}
            switchers={switchers.data ?? []}
            current={current}
            inSwitcher={inSwitcher}
            canAddSwitcher={canAddSwitcher}
            menu={menu}
          />
        }
        tabsLabel={inSwitcher ? t`Switcher` : t`Workspace`}
        tabs={
          inSwitcher && current ? (
            <SwitcherNav workspace={workspace} switcher={current} />
          ) : (
            <>
              <Link to="/$workspace/overview" params={params} className={TOP_TAB_CLASS}>
                <Trans>Overview</Trans>
              </Link>
              <Link to="/$workspace/settings/members" params={params} className={TOP_TAB_CLASS}>
                <Trans>Members</Trans>
              </Link>
              <Link to="/$workspace/settings/plan" params={params} className={TOP_TAB_CLASS}>
                <Trans>Plan</Trans>
              </Link>
              <Link to="/$workspace/settings/general" params={params} className={TOP_TAB_CLASS}>
                <Trans>Settings</Trans>
              </Link>
            </>
          )
        }
        right={
          <>
            {inSwitcher && current && (
              <ConnectChip desktop workspace={workspace} switcher={current} actor={actor} />
            )}
            {inSwitcher && current && (
              <ObsChip desktop workspaceId={workspace.id} switcher={current} />
            )}
            <UserMenu me={me} workspaceSlug={workspace.url_slug} compact />
          </>
        }
      />
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-20 flex h-15 items-center justify-between gap-3 border-b border-divider bg-background/95 px-4 backdrop-blur md:hidden">
          <button
            type="button"
            onClick={() => setSheetOpen(true)}
            className="flex h-9 min-w-0 items-center gap-2 rounded-xl border bg-card pl-1.5 pr-3 text-left font-semibold"
            aria-label={t`Switch workspace`}
          >
            <WorkspaceTile seed={workspace.url_slug} name={workspace.name} size="sm" />
            <span className="max-w-36 truncate text-sm">
              {inSwitcher && current ? current.name : workspace.name}
            </span>
            <ChevronDown className="size-4 shrink-0 text-muted-foreground" />
          </button>
          <div className="flex items-center gap-2.5">
            {inSwitcher && current && (
              <ConnectChip workspace={workspace} switcher={current} actor={actor} />
            )}
            {inSwitcher && current && <ObsChip workspaceId={workspace.id} switcher={current} />}
            <UserMenu me={me} workspaceSlug={workspace.url_slug} compact />
          </div>
        </header>

        <NoticeBanners />
        {!isSupport(actor) && <WhatsNewCard me={me} />}
        {isSupport(actor) && <SupportBanner workspace={workspace} />}
        {!isSupport(actor) && can(actor, "workspace.support_access") && (
          <EmergencyAccessBanner workspace={workspace} />
        )}
        {workspace.suspension && (
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-destructive/40 bg-destructive/10 px-4 py-2.5 text-sm md:px-8">
            <AlertTriangle className="size-4 shrink-0 text-destructive-foreground" />
            {workspace.suspension.reason ? (
              <>
                <span className="flex-1">
                  <Trans>
                    <strong>NOALBS suspended this workspace:</strong> {workspace.suspension.reason}{" "}
                    Switchers are paused and chat commands are off.
                  </Trans>
                </span>
                <a
                  href="mailto:support@noalbs.com"
                  className="font-semibold text-primary hover:underline"
                >
                  <Trans>Contact support</Trans>
                </a>
              </>
            ) : (
              // Its organization's suspension: customers aren't told why.
              <span className="flex-1">
                <Trans>
                  <strong>This workspace is paused.</strong> Switchers are paused and chat commands
                  are off.
                </Trans>
                {workspace.managed_by && (
                  <>
                    {" "}
                    <Trans>Ask {workspace.managed_by.name} about it.</Trans>
                  </>
                )}
              </span>
            )}
          </div>
        )}
        {!isSupport(actor) && (
          <ChatAccessBanners
            workspace={workspace}
            actor={actor}
            switchers={switchers.data ?? []}
            openChat={inSwitcher && segment === "chat" ? routeSwitcher : undefined}
          />
        )}
        {!pathname.endsWith("/settings/plan") && (
          <>
            <PastDueBanner workspace={workspace} />
            <MissingCardBanner workspace={workspace} />
          </>
        )}
        <main className="flex-1 pb-24 md:pb-0">{children}</main>

        <nav
          aria-label={t`Primary`}
          className="fixed inset-x-0 bottom-0 z-20 grid grid-cols-3 border-t border-divider bg-card/95 pb-[env(safe-area-inset-bottom)] backdrop-blur md:hidden"
        >
          {current ? (
            <>
              <Link
                to="/$workspace/s/$switcher/live"
                params={{ ...params, switcher: current.id }}
                className={TAB_CLASS}
              >
                <Activity className="size-5.5" />
                <Trans>Live</Trans>
              </Link>
              <Link
                to="/$workspace/s/$switcher/history"
                params={{ ...params, switcher: current.id }}
                className={TAB_CLASS}
              >
                <History className="size-5.5" />
                <Trans>History</Trans>
              </Link>
              <Link
                to="/$workspace/s/$switcher/settings"
                params={{ ...params, switcher: current.id }}
                className={cn(TAB_CLASS, settingsActive && "text-primary")}
              >
                <Settings className="size-5.5" />
                <Trans>Settings</Trans>
              </Link>
            </>
          ) : (
            <>
              <Link to="/$workspace/overview" params={params} className={TAB_CLASS}>
                <House className="size-5.5" />
                <Trans>Home</Trans>
              </Link>
              <Link to="/$workspace/settings/members" params={params} className={TAB_CLASS}>
                <Users className="size-5.5" />
                <Trans>Members</Trans>
              </Link>
              <Link to="/$workspace/settings/general" params={params} className={TAB_CLASS}>
                <Settings className="size-5.5" />
                <Trans>Settings</Trans>
              </Link>
            </>
          )}
        </nav>
      </div>

      <Sheet open={sheetOpen} onOpenChange={setSheetOpen}>
        <SheetContent side="bottom" className="max-h-[85dvh] rounded-t-2xl">
          <SheetHeader>
            <SheetTitle>
              <Trans>Workspaces</Trans>
            </SheetTitle>
            <SheetDescription className="sr-only">
              <Trans>Switch to another workspace, create one or join one.</Trans>
            </SheetDescription>
          </SheetHeader>
          <div className="flex min-h-0 flex-col gap-4 px-4 pb-6">
            <div className="flex min-h-0 flex-col gap-1.5 overflow-y-auto overscroll-contain">
              {switchers.data && switchers.data.length > 0 && (
                <div className="flex flex-col gap-1.5">
                  <span className="px-1 text-xs font-semibold text-muted-foreground">
                    <Trans>Switchers</Trans>
                  </span>
                  {switchers.data.map((candidate) => {
                    const status = switcherStatus(candidate);
                    const isCurrent = candidate.id === current?.id;
                    return (
                      <Link
                        key={candidate.id}
                        to="/$workspace/s/$switcher/live"
                        params={{ ...params, switcher: candidate.id }}
                        onClick={() => setSheetOpen(false)}
                        className={cn(
                          "flex min-h-12 items-center gap-3 rounded-xl border px-3",
                          isCurrent
                            ? "border-primary-soft-border bg-primary-soft"
                            : "border-divider bg-sunken",
                        )}
                      >
                        <StatusDot className={status.dot} />
                        <span className="flex min-w-0 flex-1 flex-col">
                          <span className="truncate font-semibold">{candidate.name}</span>
                          <span className="truncate text-xs text-muted-foreground">
                            {status.label}
                          </span>
                        </span>
                        {isCurrent && <Check className="size-5 text-primary" />}
                      </Link>
                    );
                  })}
                </div>
              )}
              <span className="px-1 pt-2 text-xs font-semibold text-muted-foreground">
                <Trans>Workspaces</Trans>
              </span>
              {menu.shown.map((candidate) => {
                const isCurrent = candidate.id === workspace.id;
                return (
                  <Link
                    key={candidate.id}
                    to="/$workspace"
                    params={{ workspace: candidate.url_slug }}
                    onClick={() => setSheetOpen(false)}
                    className={cn(
                      "flex min-h-15 items-center gap-3 rounded-xl border px-3",
                      isCurrent
                        ? "border-primary-soft-border bg-primary-soft"
                        : "border-divider bg-sunken",
                    )}
                  >
                    <WorkspaceTile seed={candidate.url_slug} name={candidate.name} size="lg" />
                    <span className="flex flex-1 flex-col">
                      <span className="font-semibold">{candidate.name}</span>
                      <span className="text-xs text-muted-foreground">
                        {standingLabel(candidate)} ·{" "}
                        {plural(candidate.switcher_count, {
                          one: "# switcher",
                          other: "# switchers",
                        })}
                        {candidate.managed_by && t` · Managed by ${candidate.managed_by.name}`}
                      </span>
                    </span>
                    {isCurrent && <Check className="size-5 text-primary" />}
                  </Link>
                );
              })}
              {organizations.data?.map((organization) => (
                <Link
                  key={organization.id}
                  to="/orgs/$organization/clients"
                  params={{ organization: organization.url_slug }}
                  onClick={() => setSheetOpen(false)}
                  className="flex min-h-15 items-center gap-3 rounded-xl border border-divider bg-sunken px-3"
                >
                  <span className="flex size-12 items-center justify-center rounded-xl bg-primary-soft text-primary-soft-foreground">
                    <Building2 className="size-5" />
                  </span>
                  <span className="flex flex-1 flex-col">
                    <span className="font-semibold">{organization.name}</span>
                    <span className="text-xs text-muted-foreground">
                      {t`Organization · ${plural(organization.usage.clients, { one: "# client", other: "# clients" })}`}
                    </span>
                  </span>
                </Link>
              ))}
            </div>
            <div className="grid grid-cols-2 gap-2.5">
              <Link
                to="/workspaces"
                onClick={() => setSheetOpen(false)}
                className="col-span-2 flex min-h-12 items-center justify-center gap-2 rounded-xl border border-divider text-sm font-medium"
              >
                <LayoutList className="size-4" />
                <Trans>All workspaces{menu.hidden > 0 && ` (${workspaces.length})`}</Trans>
              </Link>
              <Link
                to="/$workspace/overview"
                params={params}
                onClick={() => setSheetOpen(false)}
                className="flex min-h-12 items-center justify-center gap-2 rounded-xl border border-divider text-sm font-medium"
              >
                <LayoutGrid className="size-4" />
                <Trans>Overview</Trans>
              </Link>
              <Link
                to="/$workspace/settings/general"
                params={params}
                onClick={() => setSheetOpen(false)}
                className="flex min-h-12 items-center justify-center gap-2 rounded-xl border border-divider text-sm font-medium"
              >
                <Settings className="size-4" />
                <Trans>Workspace settings</Trans>
              </Link>
              <Link
                to="/welcome"
                search={{ mode: "create" }}
                onClick={() => setSheetOpen(false)}
                className="flex min-h-13 items-center justify-center gap-2 rounded-xl bg-primary font-semibold text-primary-foreground"
              >
                <Plus className="size-4.5" />
                <Trans>Create</Trans>
              </Link>
              <Link
                to="/welcome"
                search={{ mode: "join" }}
                onClick={() => setSheetOpen(false)}
                className="flex min-h-13 items-center justify-center gap-2 rounded-xl border border-border-strong font-semibold"
              >
                <Link2 className="size-4.5" />
                <Trans>Join with invite</Trans>
              </Link>
            </div>
          </div>
        </SheetContent>
      </Sheet>
    </div>
  );
}

/** The paired Connect computer; opens its OBS and program controls. */
function ConnectChip({
  workspace,
  switcher,
  actor,
  desktop = false,
}: {
  workspace: Workspace;
  switcher: SwitcherSummary;
  actor: Actor;
  desktop?: boolean;
}) {
  const details = useQuery(switcherQuery(workspace.id, switcher.id));
  const pc = switcher.status?.pc;
  if (!pc || !details.data) return null;
  const connectDeviceName = pc.device_name ?? t`Connect computer`;
  return (
    <ConnectDialog
      path={{ workspace_id: workspace.id, switcher_id: switcher.id }}
      deviceName={connectDeviceName}
      obs={pc.obs}
      streaming={Boolean(switcher.status?.streaming)}
      canControlObs={canControlConnect(details.data.connect_obs_control, actor)}
      canControlPrograms={canManageConnectPrograms(details.data.connect_programs, actor)}
      settingsLink={
        <Link
          to="/$workspace/s/$switcher/devices"
          hash="connect"
          params={{ workspace: workspace.url_slug, switcher: switcher.id }}
          className="font-semibold text-primary hover:underline"
        >
          <Trans>Connect settings</Trans>
        </Link>
      }
    >
      <button
        type="button"
        title={t`Connect · ${connectDeviceName}`}
        className={cn(
          "flex min-w-0 items-center gap-1.5 border text-muted-foreground data-[state=open]:bg-accent data-[state=open]:text-foreground",
          desktop ? "h-9 rounded-xl px-3 text-[13px]" : "h-8 rounded-full px-2.5 text-xs",
        )}
      >
        <Monitor className={cn("shrink-0", desktop ? "size-4" : "size-3.5")} />
        <span className={cn("truncate", desktop ? "max-w-40" : "max-w-20")}>
          {connectDeviceName}
        </span>
      </button>
    </ConnectDialog>
  );
}

/** Whether OBS is connected; opens OBS's stats. */
function ObsChip({
  workspaceId,
  switcher,
  desktop = false,
}: {
  workspaceId: string;
  switcher: SwitcherSummary;
  desktop?: boolean;
}) {
  const connected = Boolean(switcher.status?.obs_connected);
  return (
    <ObsStatsPopover
      path={{ workspace_id: workspaceId, switcher_id: switcher.id }}
      connected={connected}
    >
      <button
        type="button"
        title={t`OBS stats`}
        className={cn(
          "flex items-center gap-1.5 border text-muted-foreground data-[state=open]:bg-accent data-[state=open]:text-foreground",
          desktop ? "h-9 rounded-xl px-3 text-[13px]" : "h-8 rounded-full px-2.5 text-xs",
        )}
      >
        <span className={cn("size-1.5 rounded-full", connected ? "bg-live" : "bg-offline")} />
        {desktop ? (
          <>
            {connected ? <Trans>Connected</Trans> : <Trans>OBS offline</Trans>}
            <span className="sr-only">{connected ? t`connected` : t`not connected`}</span>
          </>
        ) : (
          <Trans>
            OBS
            <span className="sr-only">{connected ? t`connected` : t`not connected`}</span>
          </Trans>
        )}
      </button>
    </ObsStatsPopover>
  );
}

function WorkspaceMenu({
  workspace,
  workspaces,
  organizations,
  switchers,
  current,
  inSwitcher,
  canAddSwitcher,
  menu,
}: {
  workspace: Workspace;
  workspaces: Workspace[];
  organizations: Organization[];
  switchers: SwitcherSummary[];
  current: SwitcherSummary | undefined;
  inSwitcher: boolean;
  canAddSwitcher: boolean;
  menu: { shown: Workspace[]; hidden: number };
}) {
  const params = { workspace: workspace.url_slug };
  const [createOpen, setCreateOpen] = useState(false);

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger
          className="flex h-9 shrink-0 items-center gap-2 rounded-xl border bg-card pl-1.5 pr-3 font-semibold outline-none focus-visible:ring-2 focus-visible:ring-ring data-[state=open]:bg-accent"
          aria-label={t`Switch workspace`}
        >
          <WorkspaceTile seed={workspace.url_slug} name={workspace.name} size="sm" />
          <span className="flex max-w-60 items-center gap-2 truncate text-sm">
            <span className="truncate">{workspace.name}</span>
            {inSwitcher && current && (
              <>
                <span className="text-muted-foreground">/</span>
                <span className="truncate">{current.name}</span>
              </>
            )}
          </span>
          <ChevronDown className="size-4 shrink-0 text-muted-foreground" />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-80">
          {workspace.managed_by && workspace.organization_role && (
            <DropdownMenuItem asChild>
              <Link
                to="/orgs/$organization/clients"
                params={{ organization: workspace.managed_by.url_slug }}
              >
                <ChevronLeft />
                <Trans>Back to {workspace.managed_by.name}</Trans>
              </Link>
            </DropdownMenuItem>
          )}
          <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
            <Trans>Switchers</Trans>
          </DropdownMenuLabel>
          {switchers.map((switcher) => {
            const status = switcherStatus(switcher);
            return (
              <DropdownMenuItem key={switcher.id} asChild>
                <Link
                  to="/$workspace/s/$switcher"
                  params={{ ...params, switcher: switcher.id }}
                  className="flex min-h-10 items-center gap-2.5"
                >
                  <StatusDot className={status.dot} />
                  <span className="flex-1 truncate font-medium">{switcher.name}</span>
                  <span className={cn("font-mono text-[11px] font-medium", status.tagClass)}>
                    {status.tag}
                  </span>
                  {switcher.id === current?.id && <Check className="size-4 text-primary" />}
                </Link>
              </DropdownMenuItem>
            );
          })}
          {switchers.length === 0 && (
            <span className="block px-2 py-1.5 text-sm text-muted-foreground">
              <Trans>None yet</Trans>
            </span>
          )}
          {canAddSwitcher && (
            <DropdownMenuItem onSelect={() => setCreateOpen(true)}>
              <Plus />
              <Trans>Add switcher</Trans>
            </DropdownMenuItem>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
            {menu.hidden > 0 ? t`Recent workspaces` : t`Workspaces`}
          </DropdownMenuLabel>
          {menu.shown.map((candidate) => (
            <DropdownMenuItem key={candidate.id} asChild>
              <Link
                to="/$workspace"
                params={{ workspace: candidate.url_slug }}
                className="flex min-h-10 items-center gap-2.5"
              >
                <WorkspaceTile seed={candidate.url_slug} name={candidate.name} size="sm" />
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate font-medium leading-tight">{candidate.name}</span>
                  {candidate.managed_by && (
                    <span className="truncate text-xs text-muted-foreground">
                      <Trans>Managed by {candidate.managed_by.name}</Trans>
                    </span>
                  )}
                </span>
                <span className="text-xs text-muted-foreground">{standingLabel(candidate)}</span>
                {candidate.id === workspace.id && <Check className="size-4 text-primary" />}
              </Link>
            </DropdownMenuItem>
          ))}
          {organizations.length > 0 && (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
                <Trans>Organizations</Trans>
              </DropdownMenuLabel>
              {organizations.map((organization) => (
                <DropdownMenuItem key={organization.id} asChild>
                  <Link
                    to="/orgs/$organization/clients"
                    params={{ organization: organization.url_slug }}
                    className="flex min-h-10 items-center gap-2.5"
                  >
                    <Building2 className="size-4" />
                    <span className="flex-1 truncate font-medium">{organization.name}</span>
                    <span className="text-xs text-muted-foreground">
                      <Trans>Clients</Trans>
                    </span>
                  </Link>
                </DropdownMenuItem>
              ))}
            </>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem asChild>
            <Link to="/workspaces">
              <LayoutList />
              <Trans>All workspaces{menu.hidden > 0 && ` (${workspaces.length})`}</Trans>
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
          <DropdownMenuSeparator />
          <DropdownMenuItem asChild>
            <Link to="/$workspace/overview" params={params}>
              <LayoutGrid />
              <Trans>Workspace overview</Trans>
            </Link>
          </DropdownMenuItem>
          <DropdownMenuItem asChild>
            <Link to="/$workspace/settings/general" params={params}>
              <Settings />
              <Trans>Workspace settings</Trans>
            </Link>
          </DropdownMenuItem>
          <DropdownMenuItem asChild>
            <Link to="/$workspace/settings/members" params={params}>
              <Users />
              <Trans>Members &amp; invites</Trans>
            </Link>
          </DropdownMenuItem>
          <DropdownMenuItem asChild>
            <Link to="/$workspace/settings/plan" params={params}>
              <Trans>Plan</Trans>
              <span className="ml-auto rounded-md bg-primary-soft px-1.5 py-0.5 text-[11px] font-semibold text-primary-soft-foreground">
                {workspace.plan_code.toUpperCase()}
              </span>
            </Link>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {canAddSwitcher && (
        <CreateSwitcherDialog
          workspace={workspace}
          open={createOpen}
          onOpenChange={setCreateOpen}
        />
      )}
    </>
  );
}

export function UserMenu({
  me,
  workspaceSlug,
  compact = false,
}: {
  me: Me;
  /** The workspace the account page opens in; without one the link is left out. */
  workspaceSlug: string | null;
  compact?: boolean;
}) {
  const { preference, setPreference } = useTheme();
  const { size, setSize } = useTextSize();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const profile = profileOf(me);

  const signOut = async () => {
    await api.POST("/auth/logout");
    queryClient.clear();
    await navigate({ to: "/login" });
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        className={cn(
          "flex items-center gap-2.5 rounded-lg text-left outline-none focus-visible:ring-2 focus-visible:ring-ring",
          compact ? "min-h-11" : "w-full p-2 hover:bg-surface-2",
        )}
        aria-label={t`Account menu`}
      >
        <UserAvatar name={profile.name} avatarUrl={profile.avatarUrl} />
        {!compact && (
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="truncate text-sm font-semibold">{profile.name}</span>
            <span className="truncate text-xs text-muted-foreground">
              {profile.provider}
              {me.staff && <Trans> · Staff</Trans>}
            </span>
          </span>
        )}
      </DropdownMenuTrigger>
      <DropdownMenuContent align={compact ? "end" : "start"} side={compact ? "bottom" : "top"}>
        <DropdownMenuLabel className="flex items-center gap-2">
          {profile.name}
          {me.staff && (
            <Badge variant="secondary">
              <Trans>Staff</Trans>
            </Badge>
          )}
        </DropdownMenuLabel>
        {workspaceSlug && (
          <DropdownMenuItem asChild>
            <Link to="/$workspace/account" params={{ workspace: workspaceSlug }}>
              <UserCog />
              <Trans>Account settings</Trans>
            </Link>
          </DropdownMenuItem>
        )}
        {me.staff && (
          <DropdownMenuItem asChild>
            <Link to="/admin">
              <ShieldCheck />
              <Trans>Admin</Trans>
            </Link>
          </DropdownMenuItem>
        )}
        <DropdownMenuItem asChild>
          <a href={WHATS_NEW_PATH} target="_blank" rel="noopener">
            <Newspaper />
            <Trans>What's new</Trans>
            {hasUnseenPosts(me) && (
              <span className="ml-auto size-2 rounded-full bg-primary">
                <span className="sr-only">
                  <Trans>New</Trans>
                </span>
              </span>
            )}
          </a>
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
          <Trans>Theme</Trans>
        </DropdownMenuLabel>
        <DropdownMenuRadioGroup
          value={preference}
          onValueChange={(value) => setPreference(value as ThemePreference)}
        >
          <DropdownMenuRadioItem value="system" onSelect={(event) => event.preventDefault()}>
            <Monitor />
            <Trans>Auto</Trans>
          </DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="light" onSelect={(event) => event.preventDefault()}>
            <Sun />
            <Trans>Light</Trans>
          </DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="dark" onSelect={(event) => event.preventDefault()}>
            <Moon />
            <Trans>Dark</Trans>
          </DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
        <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
          <Trans>Text size</Trans>
        </DropdownMenuLabel>
        <DropdownMenuRadioGroup value={size} onValueChange={(value) => setSize(value as TextSize)}>
          <DropdownMenuRadioItem value="default" onSelect={(event) => event.preventDefault()}>
            <Trans>Default</Trans>
          </DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="large" onSelect={(event) => event.preventDefault()}>
            <Trans>Large</Trans>
          </DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="larger" onSelect={(event) => event.preventDefault()}>
            <Trans>Larger</Trans>
          </DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
        <LanguageMenu />
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={signOut}>
          <LogOut />
          <Trans>Sign out</Trans>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * The open (or last used) switcher's pages. While its setup is unfinished a
 * "Finish setup" item leads the list; afterwards Setup is reached from Settings.
 */
function SwitcherNav({ workspace, switcher }: { workspace: Workspace; switcher: SwitcherSummary }) {
  const details = useQuery(switcherQuery(workspace.id, switcher.id));
  const params = { workspace: workspace.url_slug, switcher: switcher.id };
  const progress = details.data
    ? setupProgress({
        connected: Boolean(switcher.status?.obs_connected) || switcher.last_seen_at != null,
        serverCount: switcher.stream_server_count,
        scenes: details.data.scenes,
      })
    : null;

  return (
    <>
      {progress && !progress.complete && (
        <Link
          to="/$workspace/s/$switcher/setup"
          params={params}
          className={cn(TOP_TAB_CLASS, "gap-1.5 text-primary hover:text-primary")}
        >
          <span>
            <Trans>Finish setup</Trans>
          </span>{" "}
          <span className="font-mono text-[11px] tabular">
            {progress.done}/{progress.total}
          </span>
        </Link>
      )}
      {switcherPages().map((page) => (
        <Link key={page.to} to={page.to} params={params} className={TOP_TAB_CLASS}>
          {page.label}
        </Link>
      ))}
    </>
  );
}

/** A warning about a chat grant, with the way to the page that connects it again. */
export function ChatAccessBanner({ children, link }: { children: ReactNode; link: ReactNode }) {
  return (
    <div
      role="alert"
      className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-low/40 bg-low/10 px-4 py-2.5 text-sm md:px-8"
    >
      <AlertTriangle className="size-4 shrink-0 text-low" />
      <span className="flex-1">{children}</span>
      {link}
    </div>
  );
}

export const RECONNECT_LINK = "font-semibold text-primary hover:underline";

/**
 * Tells whoever can reconnect them which chat grants the platform refused. The
 * Chat page says so itself, so the one on screen isn't repeated.
 */
function ChatAccessBanners({
  workspace,
  actor,
  switchers,
  openChat,
}: {
  workspace: Workspace;
  actor: Actor;
  switchers: SwitcherSummary[];
  /** The switcher whose Chat page is open. */
  openChat: string | undefined;
}) {
  const canConnectBots = can(actor, "account.manage");
  const bots = useQuery({ ...chatBotsQuery(workspace.id), enabled: canConnectBots });
  const params = { workspace: workspace.url_slug };

  const channels = can(actor, "switcher.manage") ? switchersToReconnect(switchers, openChat) : [];
  const firstSwitcher = channels[0];
  // Any switcher's Chat page has the workspace's bots.
  const botSwitcher = switchers[0];
  const shownBots = canConnectBots && !openChat ? bots.data : undefined;

  return (
    <>
      {firstSwitcher && (
        <ChatAccessBanner
          link={
            <Link
              to="/$workspace/s/$switcher/chat"
              params={{ ...params, switcher: firstSwitcher.id }}
              className={RECONNECT_LINK}
            >
              <Trans>Reconnect</Trans>
            </Link>
          }
        >
          {channels.length > 1
            ? plural(channels.length, {
                other:
                  "Channels on # switchers need reconnecting: the bot lost its access, so commands from their chats stopped working.",
              })
            : t`A channel on ${firstSwitcher.name} needs reconnecting: the bot lost its access, so commands from its chat stopped working.`}
        </ChatAccessBanner>
      )}
      {botSwitcher &&
        shownBots &&
        revokedOwnBots(shownBots).map((bot) => (
          <ChatAccessBanner
            key={bot.platform}
            link={
              <Link
                to="/$workspace/s/$switcher/chat"
                params={{ ...params, switcher: botSwitcher.id }}
                className={RECONNECT_LINK}
              >
                <Trans>Reconnect</Trans>
              </Link>
            }
          >
            {botDetail(
              bot,
              speakingBot(shownBots, bot.platform),
              true,
              workspace.managed_by?.name ?? null,
            )}
          </ChatAccessBanner>
        ))}
    </>
  );
}

/** Tells whoever pays that a payment failed, while Stripe retries it. */
function PastDueBanner({ workspace }: { workspace: Workspace }) {
  const plan = useQuery(workspacePlanQuery(workspace.id));
  if (!plan.data?.can_manage_billing || plan.data.subscription?.status !== "past_due") return null;

  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-low/40 bg-low/10 px-4 py-2.5 text-sm md:px-8">
      <AlertTriangle className="size-4 shrink-0 text-low" />
      <span className="flex-1">
        <Trans>
          A payment for the {plan.data.plan.name} plan failed. Update your card to keep it.
        </Trans>
      </span>
      <Link
        to="/$workspace/settings/plan"
        params={{ workspace: workspace.url_slug }}
        className="font-semibold text-primary hover:underline"
      >
        <Trans>Manage billing</Trans>
      </Link>
    </div>
  );
}

/** Staff in a workspace that isn't theirs: everything they do is shown to it. */
function SupportBanner({ workspace }: { workspace: Workspace }) {
  const grant = workspace.support_access;
  if (!grant) return null;
  const name = workspace.name;
  const level = supportLevelLabels()[grant.level];
  const until = new Date(grant.expires_at).toLocaleString(currentLocale());

  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-primary/30 bg-primary-soft px-4 py-2.5 text-sm text-primary-soft-foreground md:px-8">
      <Headset className="size-4 shrink-0" />
      <span className="flex-1">
        {grant.break_glass
          ? t`You’re in ${name} as NOALBS support, with emergency access (${level}) until ${until}. Everything you do here is shown to the workspace.`
          : t`You’re in ${name} as NOALBS support, with its permission (${level}) until ${until}. Everything you do here is shown to the workspace.`}
      </span>
    </div>
  );
}

/** Owners and admins: NOALBS staff opened the workspace without permission. */
function EmergencyAccessBanner({ workspace }: { workspace: Workspace }) {
  const access = useQuery(supportAccessQuery(workspace.id));
  const unseen = access.data?.emergency.filter((emergency) => !emergency.acknowledged) ?? [];
  const latest = unseen[0];
  if (!latest) return null;
  const level = supportLevelLabels()[latest.level];
  const { staff_name: staff, reason } = latest;

  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-low/40 bg-low/10 px-4 py-2.5 text-sm md:px-8">
      <AlertTriangle className="size-4 shrink-0 text-low" />
      <span className="flex-1">
        {staff
          ? t`NOALBS support opened this workspace without your permission (${level}, ${staff}): “${reason}”.`
          : t`NOALBS support opened this workspace without your permission (${level}): “${reason}”.`}
        {unseen.length > 1 &&
          ` ${plural(unseen.length - 1, { one: "It happened # more time.", other: "It happened # more times." })}`}
      </span>
      <Link
        to="/$workspace/settings/general"
        params={{ workspace: workspace.url_slug }}
        hash="support-access"
        className="font-semibold text-primary hover:underline"
      >
        <Trans>Review</Trans>
      </Link>
    </div>
  );
}

/** After an ownership transfer: the new owner has to add a card before the renewal. */
function MissingCardBanner({ workspace }: { workspace: Workspace }) {
  const plan = useQuery(workspacePlanQuery(workspace.id));
  const reminder = plan.data?.can_manage_billing ? missingCardReminder(plan.data) : null;
  if (!reminder) return null;

  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-low/40 bg-low/10 px-4 py-2.5 text-sm md:px-8">
      <AlertTriangle className="size-4 shrink-0 text-low" />
      <span className="flex-1">{reminder}</span>
      <Link
        to="/$workspace/settings/plan"
        params={{ workspace: workspace.url_slug }}
        className="font-semibold text-primary hover:underline"
      >
        <Trans>Manage billing</Trans>
      </Link>
    </div>
  );
}
