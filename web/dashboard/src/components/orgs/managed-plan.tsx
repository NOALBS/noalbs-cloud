import { useQuery } from "@tanstack/react-query";
import { workspacePlanQuery } from "@/api/queries";
import type { Workspace } from "@/api/types";
import { Page, Section } from "@/components/page";
import { Skeleton } from "@/components/ui/skeleton";
import { UsageBar } from "@/components/usage-bar";

/** The Plan page of a client workspace: its organization pays, so there is nothing to buy. */
export function ManagedPlan({ workspace }: { workspace: Workspace }) {
  const plan = useQuery(workspacePlanQuery(workspace.id));
  const organization = workspace.managed_by;

  return (
    <Page title="Plan" description="What this workspace can use.">
      <Section
        title={organization ? `Limits set by ${organization.name}` : "Managed workspace"}
        description="What this workspace can use is decided by the organization that manages it."
      >
        {!plan.data ? (
          <Skeleton className="mx-5 mb-5 h-28" />
        ) : (
          <div className="flex flex-col gap-4 px-5 pb-5">
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
            <dl className="grid gap-2 text-sm sm:grid-cols-3">
              <div>
                <dt className="text-muted-foreground">Stream history</dt>
                <dd className="font-semibold">{plan.data.plan.history_days} days</dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Stream servers per switcher</dt>
                <dd className="font-semibold">{plan.data.plan.max_stream_servers}</dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Own chat bot</dt>
                <dd className="font-semibold">{plan.data.plan.custom_chat_bot ? "Yes" : "No"}</dd>
              </div>
            </dl>
          </div>
        )}
      </Section>
    </Page>
  );
}
