import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { ArrowRight, Plus, Radio, UserPlus } from "lucide-react";
import { membersQuery, switchersQuery, workspacePlanQuery } from "@/api/queries";
import { CreateSwitcherDialog } from "@/components/create-switcher-dialog";
import { Page, Section } from "@/components/page";
import { StatusDot, switcherStatus } from "@/components/switcher-status";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { UsageBar } from "@/components/usage-bar";
import { UserAvatar } from "@/components/user-avatar";
import { useWorkspace } from "@/hooks/use-workspace";
import { can } from "@/lib/permissions";

export const Route = createFileRoute("/_app/$workspace/overview")({
  component: WorkspaceHome,
});

function WorkspaceHome() {
  const { workspace, actor } = useWorkspace();
  const members = useQuery(membersQuery(workspace.id));
  const plan = useQuery(workspacePlanQuery(workspace.id));
  const switchers = useQuery(switchersQuery(workspace.id));
  const canAddSwitcher = can(actor, "switcher.manage");
  const params = { workspace: workspace.url_slug };

  return (
    <Page title={workspace.name} description="Your switchers, team and plan at a glance.">
      <Section
        title="Switchers"
        description="A switcher connects one OBS to your stream servers and switches scenes for you."
        actions={
          canAddSwitcher &&
          (switchers.data?.length ?? 0) > 0 && (
            <CreateSwitcherDialog workspace={workspace}>
              <Button variant="outline" size="sm">
                <Plus />
                Add switcher
              </Button>
            </CreateSwitcherDialog>
          )
        }
      >
        {switchers.isPending && <Skeleton className="mx-5 mb-5 h-16 rounded-xl" />}
        {switchers.data?.length === 0 && (
          <div className="mx-5 mb-5 flex flex-col items-center gap-3 rounded-xl border border-dashed px-6 py-10 text-center">
            <span className="flex size-11 items-center justify-center rounded-xl bg-primary-soft text-primary-soft-foreground">
              <Radio className="size-5" />
            </span>
            <p className="font-semibold">No switchers yet</p>
            <p className="max-w-sm text-sm text-muted-foreground">
              {canAddSwitcher
                ? "Add one to connect your OBS. Setup takes a couple of minutes."
                : "An admin of this workspace can add one."}
            </p>
            {canAddSwitcher && (
              <CreateSwitcherDialog workspace={workspace}>
                <Button>
                  <Plus />
                  Add switcher
                </Button>
              </CreateSwitcherDialog>
            )}
          </div>
        )}
        {switchers.data && switchers.data.length > 0 && (
          <ul className="flex flex-col">
            {switchers.data.map((switcher) => {
              const status = switcherStatus(switcher);
              return (
                <li key={switcher.id} className="border-t border-divider">
                  <Link
                    to="/$workspace/s/$switcher"
                    params={{ ...params, switcher: switcher.id }}
                    className="flex min-h-16 items-center gap-3 px-5 py-3 hover:bg-surface-2/50"
                  >
                    <StatusDot className={status.dot} />
                    <span className="flex flex-1 flex-col gap-0.5">
                      <span className="font-semibold">{switcher.name}</span>
                      <span className="text-xs text-muted-foreground">
                        {status.label} ·{" "}
                        {switcher.stream_server_count === 1
                          ? "1 stream server"
                          : `${switcher.stream_server_count} stream servers`}
                      </span>
                    </span>
                    <ArrowRight className="size-4 text-muted-foreground" />
                  </Link>
                </li>
              );
            })}
          </ul>
        )}
      </Section>

      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <Section
          title="Team"
          actions={
            can(actor, "account.manage") && (
              <Button asChild variant="outline" size="sm">
                <Link to="/$workspace/settings/members" params={params}>
                  <UserPlus />
                  Invite
                </Link>
              </Button>
            )
          }
        >
          <div className="flex flex-col gap-3 px-5 pb-5">
            {members.isPending ? (
              <Skeleton className="h-9 w-40" />
            ) : (
              <div className="flex -space-x-2">
                {members.data?.slice(0, 8).map((member) => (
                  <UserAvatar
                    key={member.user_id}
                    name={member.username}
                    avatarUrl={member.avatar_url}
                    className="size-9 ring-2 ring-card"
                  />
                ))}
              </div>
            )}
            <Link
              to="/$workspace/settings/members"
              params={params}
              className="flex items-center gap-1.5 text-sm font-medium text-primary hover:underline"
            >
              {members.data?.length === 1 ? "1 member" : `${members.data?.length ?? "…"} members`}
              <ArrowRight className="size-3.5" />
            </Link>
          </div>
        </Section>

        <Section title="Plan">
          <div className="flex flex-col gap-4 px-5 pb-5">
            {plan.data ? (
              <>
                <UsageBar
                  label="Switchers"
                  used={plan.data.usage.switchers}
                  max={plan.data.plan.max_switchers}
                />
                <UsageBar
                  label="Members"
                  used={plan.data.usage.members}
                  max={plan.data.plan.max_members}
                />
              </>
            ) : (
              <Skeleton className="h-16" />
            )}
            <Link
              to="/$workspace/settings/plan"
              params={params}
              className="flex items-center gap-1.5 text-sm font-medium text-primary hover:underline"
            >
              {plan.data?.plan.name ?? "…"} plan
              <ArrowRight className="size-3.5" />
            </Link>
          </div>
        </Section>
      </div>
    </Page>
  );
}
