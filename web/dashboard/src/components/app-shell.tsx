import { Trans, useLingui } from "@lingui/react/macro";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useLocation, useNavigate, useParams } from "@tanstack/react-router";
import {
  Activity,
  AlertTriangle,
  Building2,
  Check,
  ChevronLeft,
  ChevronsUpDown,
  Headset,
  History,
  House,
  LayoutGrid,
  LayoutList,
  Link2,
  LogOut,
  Monitor,
  Moon,
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
  organizationsQuery,
  supportAccessQuery,
  switcherQuery,
  switchersQuery,
  workspacePlanQuery,
} from "@/api/queries";
import type { Me, SwitcherSummary, Workspace } from "@/api/types";
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
import { missingCardReminder } from "@/lib/billing";
import { lastSwitcherId } from "@/lib/last-switcher";
import { currentLocale } from "@/lib/locale";
import { actorOf, can, isSupport, ROLE_LABELS } from "@/lib/permissions";
import { menuWorkspaces, recentWorkspaceIds } from "@/lib/recent-workspaces";
import { setupProgress } from "@/lib/setup";
import { SUPPORT_LEVEL_LABELS } from "@/lib/support";
import { type ThemePreference, useTheme } from "@/lib/theme";
import { cn } from "@/lib/utils";
import { CreateSwitcherDialog } from "./create-switcher-dialog";
import { LanguageMenu } from "./language-menu";
import { ObsStatsPopover } from "./live/obs-stats-popover";
import { SETTINGS_SEGMENTS, SWITCHER_PAGES } from "./switcher-page";
import { StatusDot, switcherStatus } from "./switcher-status";
import { providerLabel, UserAvatar } from "./user-avatar";
import { WorkspaceTile } from "./workspace-tile";

export const NAV_LINK =
  "flex min-h-9.5 items-center gap-2.5 rounded-lg px-2.5 text-sm font-medium text-muted-foreground hover:bg-surface-2 hover:text-foreground data-[status=active]:bg-surface-2 data-[status=active]:font-semibold data-[status=active]:text-foreground";

export const SECTION_LABEL =
  "px-2.5 pb-1 text-[11px] font-semibold tracking-wider text-subtle-foreground";

export const TAB_CLASS =
  "flex h-17 flex-col items-center justify-center gap-1 text-xs font-medium text-muted-foreground data-[status=active]:font-semibold data-[status=active]:text-foreground";
const TAB_ACTIVE = "font-semibold text-foreground";

/** What to call the viewer's standing in a workspace of the menu. */
export function standingLabel(workspace: Workspace): string {
  if (workspace.role) return ROLE_LABELS[workspace.role];
  if (workspace.organization_role) return "Organization";
  return "Staff";
}

function profileOf(me: Me) {
  const connection = me.connections[0];
  return {
    name: connection?.username ?? "Account",
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
    <div className="flex min-h-dvh">
      <aside className="sticky top-0 hidden h-dvh w-66 shrink-0 flex-col gap-5 overflow-y-auto border-r border-divider bg-sunken px-3 py-3.5 md:flex">
        <WorkspaceSwitcher workspace={workspace} workspaces={workspaces} />

        {workspace.managed_by && workspace.organization_role && (
          <Link
            to="/orgs/$organization/clients"
            params={{ organization: workspace.managed_by.url_slug }}
            className={cn(NAV_LINK, "-mt-2")}
          >
            <ChevronLeft className="size-4" />
            <span className="truncate">{workspace.managed_by.name}</span>
          </Link>
        )}

        <nav aria-label="Switchers" className="flex flex-col gap-0.5">
          <span className={cn(SECTION_LABEL, "flex items-center justify-between")}>
            SWITCHERS
            {canAddSwitcher && (
              <CreateSwitcherDialog workspace={workspace}>
                <button
                  type="button"
                  aria-label="Add switcher"
                  className="rounded p-0.5 text-muted-foreground hover:bg-surface-2 hover:text-foreground"
                >
                  <Plus className="size-3.5" />
                </button>
              </CreateSwitcherDialog>
            )}
          </span>
          {switchers.data?.map((switcher) => {
            const status = switcherStatus(switcher);
            return (
              <Link
                key={switcher.id}
                to="/$workspace/s/$switcher"
                params={{ ...params, switcher: switcher.id }}
                className={NAV_LINK}
              >
                <StatusDot className={status.dot} />
                <span className="flex-1 truncate">{switcher.name}</span>
                <span className={cn("font-mono text-[11px] font-medium", status.tagClass)}>
                  {status.tag}
                </span>
              </Link>
            );
          })}
          {switchers.data?.length === 0 && (
            <span className="px-2.5 text-sm text-muted-foreground">None yet</span>
          )}
        </nav>

        {current && <SwitcherNav workspace={workspace} switcher={current} />}

        <nav aria-label="Workspace" className="flex flex-col gap-0.5">
          <span className={SECTION_LABEL}>WORKSPACE</span>
          <Link to="/$workspace/settings/members" params={params} className={NAV_LINK}>
            Members &amp; invites
          </Link>
          <Link to="/$workspace/settings/plan" params={params} className={NAV_LINK}>
            <span className="flex-1">Plan</span>
            <span className="rounded-md bg-primary-soft px-1.5 py-0.5 text-[11px] font-semibold text-primary-soft-foreground">
              {workspace.plan_code.toUpperCase()}
            </span>
          </Link>
        </nav>

        <div className="mt-auto border-t border-divider pt-3">
          <UserMenu me={me} workspaceSlug={workspace.url_slug} />
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-20 flex h-15 items-center justify-between gap-3 border-b border-divider bg-background/95 px-4 backdrop-blur md:hidden">
          <button
            type="button"
            onClick={() => setSheetOpen(true)}
            className="flex min-h-11 min-w-0 items-center gap-2.5 text-left"
            aria-label="Switch workspace"
          >
            <WorkspaceTile seed={workspace.url_slug} name={workspace.name} />
            <span className="flex min-w-0 flex-col">
              <span className="truncate text-[15px] leading-tight font-semibold">
                {workspace.name}
              </span>
              {inSwitcher && current ? (
                <span className="truncate text-xs text-muted-foreground">{current.name}</span>
              ) : (
                workspace.managed_by && (
                  <span className="truncate text-xs text-muted-foreground">
                    Managed by {workspace.managed_by.name}
                  </span>
                )
              )}
            </span>
            <ChevronsUpDown className="size-4 shrink-0 text-muted-foreground" />
          </button>
          <div className="flex items-center gap-2.5">
            {inSwitcher && current && <ObsChip workspaceId={workspace.id} switcher={current} />}
            <UserMenu me={me} workspaceSlug={workspace.url_slug} compact />
          </div>
        </header>

        <NoticeBanners />
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
                  <strong>NOALBS suspended this workspace:</strong> {workspace.suspension.reason}{" "}
                  Switchers are paused and chat commands are off.
                </span>
                <a
                  href="mailto:support@noalbs.com"
                  className="font-semibold text-primary hover:underline"
                >
                  Contact support
                </a>
              </>
            ) : (
              // Its organization's suspension: customers aren't told why.
              <span className="flex-1">
                <strong>This workspace is paused.</strong> Switchers are paused and chat commands
                are off.
                {workspace.managed_by && <> Ask {workspace.managed_by.name} about it.</>}
              </span>
            )}
          </div>
        )}
        {!pathname.endsWith("/settings/plan") && (
          <>
            <PastDueBanner workspace={workspace} />
            <MissingCardBanner workspace={workspace} />
          </>
        )}
        <main className="flex-1 pb-24 md:pb-0">{children}</main>

        <nav
          aria-label="Primary"
          className={cn(
            "fixed inset-x-0 bottom-0 z-20 grid grid-cols-3 border-t border-divider bg-sunken pb-[env(safe-area-inset-bottom)] md:hidden",
          )}
        >
          {current ? (
            <>
              <Link
                to="/$workspace/s/$switcher/live"
                params={{ ...params, switcher: current.id }}
                className={TAB_CLASS}
              >
                <Activity className="size-5.5" />
                Live
              </Link>
              <Link
                to="/$workspace/s/$switcher/history"
                params={{ ...params, switcher: current.id }}
                className={TAB_CLASS}
              >
                <History className="size-5.5" />
                History
              </Link>
              <Link
                to="/$workspace/s/$switcher/triggers"
                params={{ ...params, switcher: current.id }}
                className={cn(TAB_CLASS, settingsActive && TAB_ACTIVE)}
              >
                <Settings className="size-5.5" />
                Settings
              </Link>
            </>
          ) : (
            <>
              <Link to="/$workspace/overview" params={params} className={TAB_CLASS}>
                <House className="size-5.5" />
                Home
              </Link>
              <Link to="/$workspace/settings/members" params={params} className={TAB_CLASS}>
                <Users className="size-5.5" />
                Members
              </Link>
              <Link to="/$workspace/settings/general" params={params} className={TAB_CLASS}>
                <Settings className="size-5.5" />
                Settings
              </Link>
            </>
          )}
        </nav>
      </div>

      <Sheet open={sheetOpen} onOpenChange={setSheetOpen}>
        <SheetContent side="bottom" className="max-h-[85dvh] rounded-t-2xl">
          <SheetHeader>
            <SheetTitle>Workspaces</SheetTitle>
            <SheetDescription className="sr-only">
              Switch to another workspace, create one or join one.
            </SheetDescription>
          </SheetHeader>
          <div className="flex min-h-0 flex-col gap-4 px-4 pb-6">
            <div className="flex min-h-0 flex-col gap-1.5 overflow-y-auto overscroll-contain">
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
                        {candidate.switcher_count === 1
                          ? "1 switcher"
                          : `${candidate.switcher_count} switchers`}
                        {candidate.managed_by && ` · Managed by ${candidate.managed_by.name}`}
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
                      Organization · {organization.usage.clients}{" "}
                      {organization.usage.clients === 1 ? "client" : "clients"}
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
                All workspaces{menu.hidden > 0 && ` (${workspaces.length})`}
              </Link>
              <Link
                to="/$workspace/overview"
                params={params}
                onClick={() => setSheetOpen(false)}
                className="flex min-h-12 items-center justify-center gap-2 rounded-xl border border-divider text-sm font-medium"
              >
                <LayoutGrid className="size-4" />
                Overview
              </Link>
              <Link
                to="/$workspace/settings/general"
                params={params}
                onClick={() => setSheetOpen(false)}
                className="flex min-h-12 items-center justify-center gap-2 rounded-xl border border-divider text-sm font-medium"
              >
                <Settings className="size-4" />
                Workspace settings
              </Link>
              <Link
                to="/welcome"
                search={{ mode: "create" }}
                onClick={() => setSheetOpen(false)}
                className="flex min-h-13 items-center justify-center gap-2 rounded-xl bg-primary font-semibold text-primary-foreground"
              >
                <Plus className="size-4.5" />
                Create
              </Link>
              <Link
                to="/welcome"
                search={{ mode: "join" }}
                onClick={() => setSheetOpen(false)}
                className="flex min-h-13 items-center justify-center gap-2 rounded-xl border border-border-strong font-semibold"
              >
                <Link2 className="size-4.5" />
                Join with invite
              </Link>
            </div>
          </div>
        </SheetContent>
      </Sheet>
    </div>
  );
}

/** Whether OBS is connected, for the mobile app bar; opens OBS's stats. */
function ObsChip({ workspaceId, switcher }: { workspaceId: string; switcher: SwitcherSummary }) {
  const connected = Boolean(switcher.status?.obs_connected);
  return (
    <ObsStatsPopover
      path={{ workspace_id: workspaceId, switcher_id: switcher.id }}
      connected={connected}
    >
      <button
        type="button"
        title="OBS stats"
        className="flex h-8 items-center gap-1.5 rounded-full border px-2.5 text-xs text-muted-foreground data-[state=open]:bg-accent data-[state=open]:text-foreground"
      >
        <span className={cn("size-1.5 rounded-full", connected ? "bg-live" : "bg-offline")} />
        OBS
        <span className="sr-only">{connected ? "connected" : "not connected"}</span>
      </button>
    </ObsStatsPopover>
  );
}

/** The desktop workspace switcher: the current workspace, opening a menu. */
function WorkspaceSwitcher({
  workspace,
  workspaces,
}: {
  workspace: Workspace;
  workspaces: Workspace[];
}) {
  const params = { workspace: workspace.url_slug };
  const organizations = useQuery(organizationsQuery);
  // With many workspaces the menu lists the recent ones; the rest are on All workspaces.
  const menu = menuWorkspaces(workspaces, workspace.id, recentWorkspaceIds());

  return (
    <DropdownMenu>
      <DropdownMenuTrigger className="flex h-12 shrink-0 items-center gap-2.5 rounded-lg px-2.5 text-left outline-none hover:bg-surface-2 focus-visible:ring-2 focus-visible:ring-ring data-[state=open]:bg-surface-2">
        <WorkspaceTile seed={workspace.url_slug} name={workspace.name} />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate font-semibold leading-tight">{workspace.name}</span>
          {workspace.managed_by && (
            <span className="truncate text-xs text-muted-foreground">
              Managed by {workspace.managed_by.name}
            </span>
          )}
        </span>
        <ChevronsUpDown className="size-4 text-muted-foreground" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-75">
        <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
          {menu.hidden > 0 ? "Recent workspaces" : "Workspaces"}
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
                    Managed by {candidate.managed_by.name}
                  </span>
                )}
              </span>
              <span className="text-xs text-muted-foreground">{standingLabel(candidate)}</span>
              {candidate.id === workspace.id && <Check className="size-4 text-primary" />}
            </Link>
          </DropdownMenuItem>
        ))}
        {organizations.data && organizations.data.length > 0 && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
              Organizations
            </DropdownMenuLabel>
            {organizations.data.map((organization) => (
              <DropdownMenuItem key={organization.id} asChild>
                <Link
                  to="/orgs/$organization/clients"
                  params={{ organization: organization.url_slug }}
                  className="flex min-h-10 items-center gap-2.5"
                >
                  <Building2 className="size-4" />
                  <span className="flex-1 truncate font-medium">{organization.name}</span>
                  <span className="text-xs text-muted-foreground">Clients</span>
                </Link>
              </DropdownMenuItem>
            ))}
          </>
        )}
        <DropdownMenuSeparator />
        <DropdownMenuItem asChild>
          <Link to="/workspaces">
            <LayoutList />
            All workspaces{menu.hidden > 0 && ` (${workspaces.length})`}
          </Link>
        </DropdownMenuItem>
        <DropdownMenuItem asChild>
          <Link to="/welcome" search={{ mode: "create" }}>
            <Plus />
            Create workspace
          </Link>
        </DropdownMenuItem>
        <DropdownMenuItem asChild>
          <Link to="/welcome" search={{ mode: "join" }}>
            <Link2 />
            Join with invite link
          </Link>
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem asChild>
          <Link to="/$workspace/overview" params={params}>
            <LayoutGrid />
            Workspace overview
          </Link>
        </DropdownMenuItem>
        <DropdownMenuItem asChild>
          <Link to="/$workspace/settings/general" params={params}>
            <Settings />
            Workspace settings
          </Link>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
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
  const { t } = useLingui();
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
    <nav aria-label="Switcher" className="flex flex-col gap-0.5">
      {progress && !progress.complete && (
        <Link
          to="/$workspace/s/$switcher/setup"
          params={params}
          className={cn(NAV_LINK, "mb-1 text-primary hover:text-primary")}
        >
          <span className="flex-1">Finish setup</span>
          <span className="font-mono text-[11px] tabular">
            {progress.done}/{progress.total}
          </span>
        </Link>
      )}
      {SWITCHER_PAGES.map((page) => (
        <Link key={page.to} to={page.to} params={params} className={NAV_LINK}>
          {page.label}
        </Link>
      ))}
    </nav>
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
        A payment for the {plan.data.plan.name} plan failed. Update your card to keep it.
      </span>
      <Link
        to="/$workspace/settings/plan"
        params={{ workspace: workspace.url_slug }}
        className="font-semibold text-primary hover:underline"
      >
        Manage billing
      </Link>
    </div>
  );
}

/** Staff in a workspace that isn't theirs: everything they do is shown to it. */
function SupportBanner({ workspace }: { workspace: Workspace }) {
  const grant = workspace.support_access;
  if (!grant) return null;

  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-primary/30 bg-primary-soft px-4 py-2.5 text-sm text-primary-soft-foreground md:px-8">
      <Headset className="size-4 shrink-0" />
      <span className="flex-1">
        You’re in {workspace.name} as NOALBS support,{" "}
        {grant.break_glass
          ? `with emergency access (${SUPPORT_LEVEL_LABELS[grant.level].toLowerCase()})`
          : `with its permission (${SUPPORT_LEVEL_LABELS[grant.level].toLowerCase()})`}{" "}
        until {new Date(grant.expires_at).toLocaleString(currentLocale())}. Everything you do here
        is shown to the workspace.
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

  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-low/40 bg-low/10 px-4 py-2.5 text-sm md:px-8">
      <AlertTriangle className="size-4 shrink-0 text-low" />
      <span className="flex-1">
        NOALBS support opened this workspace without your permission (
        {SUPPORT_LEVEL_LABELS[latest.level].toLowerCase()}
        {latest.staff_name && `, ${latest.staff_name}`}): “{latest.reason}”
        {unseen.length > 1 && ` and ${unseen.length - 1} more time(s)`}.
      </span>
      <Link
        to="/$workspace/settings/general"
        params={{ workspace: workspace.url_slug }}
        hash="support-access"
        className="font-semibold text-primary hover:underline"
      >
        Review
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
        Manage billing
      </Link>
    </div>
  );
}
