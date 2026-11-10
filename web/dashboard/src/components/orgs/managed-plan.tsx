import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
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
    <Page title={t`Plan`} description={t`What this workspace can use.`}>
      <Section
        title={organization ? t`Limits set by ${organization.name}` : t`Managed workspace`}
        description={t`What this workspace can use is decided by the organization that manages it.`}
      >
        {!plan.data ? (
          <Skeleton className="mx-5 mb-5 h-28" />
        ) : (
          <div className="flex flex-col">
            <div className="grid gap-5 border-t border-divider px-5 py-3 sm:grid-cols-2">
              <UsageBar
                label={t`Switchers`}
                used={plan.data.usage.switchers}
                max={plan.data.plan.max_switchers}
              />
              <UsageBar
                label={t`Members`}
                used={plan.data.usage.members}
                max={plan.data.plan.max_members}
              />
            </div>
            <dl className="grid border-t border-divider px-5 py-3 text-sm sm:grid-cols-3 [&>div]:py-2 [&_dd]:font-display [&_dd]:tabular-nums">
              <div>
                <dt className="text-muted-foreground">
                  <Trans>Stream history</Trans>
                </dt>
                <dd className="font-semibold">
                  <Trans>{plan.data.plan.history_days} days</Trans>
                </dd>
              </div>
              <div>
                <dt className="text-muted-foreground">
                  <Trans>Stream servers per switcher</Trans>
                </dt>
                <dd className="font-semibold">{plan.data.plan.max_stream_servers}</dd>
              </div>
              <div>
                <dt className="text-muted-foreground">
                  <Trans>Own chat bot</Trans>
                </dt>
                <dd className="font-semibold">{plan.data.plan.custom_chat_bot ? t`Yes` : t`No`}</dd>
              </div>
            </dl>
          </div>
        )}
      </Section>
    </Page>
  );
}
