import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { membersQuery, queryKeys, workspacePlanQuery } from "@/api/queries";
import { Section } from "@/components/page";
import { SettingsRow } from "@/components/settings-row";
import { TypeToConfirm } from "@/components/type-to-confirm";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useWorkspace } from "@/hooks/use-workspace";
import { currentLocale } from "@/lib/locale";
import { roleLabels, transferCandidates } from "@/lib/permissions";

function formatDay(date: string): string {
  return new Date(date).toLocaleDateString(currentLocale(), {
    day: "numeric",
    month: "long",
    year: "numeric",
  });
}

/**
 * Hands the workspace to an admin (staff: any member). The owner stays as an
 * admin; with Stripe, their cards and billing details leave the workspace.
 */
export function TransferOwnership() {
  const { workspace, actor } = useWorkspace();
  const queryClient = useQueryClient();
  const members = useQuery(membersQuery(workspace.id));
  const plan = useQuery(workspacePlanQuery(workspace.id));
  const [chosen, setChosen] = useState<string>("");

  const candidates = transferCandidates(actor, members.data ?? []);
  const target = candidates.find((member) => member.user_id === chosen);
  const targetName = target?.username ?? t`this member`;
  const subscription = plan.data?.subscription;
  const planName = plan.data?.plan.name ?? "";

  return (
    <Section title={t`Transfer ownership`}>
      <SettingsRow
        label={t`New owner`}
        description={
          actor.role !== "owner"
            ? t`Make another member the owner. The current owner stays as an admin.`
            : t`Make an admin the owner, for example so you can leave. You stay as an admin.`
        }
      >
        {members.data && candidates.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            <Trans>
              Only an admin can become the owner.{" "}
              <Link
                to="/$workspace/settings/members"
                params={{ workspace: workspace.url_slug }}
                className="font-medium text-primary hover:underline"
              >
                Make someone an admin
              </Link>{" "}
              first.
            </Trans>
          </p>
        ) : (
          <>
            <Select value={chosen} onValueChange={setChosen}>
              <SelectTrigger className="w-56" aria-label={t`New owner`}>
                <SelectValue placeholder={t`Choose the new owner`} />
              </SelectTrigger>
              <SelectContent>
                {candidates.map((member) => (
                  <SelectItem key={member.user_id} value={member.user_id}>
                    {member.username ?? t`Unnamed`}
                    {actor.staff && ` (${roleLabels()[member.role]})`}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <TypeToConfirm
              trigger={t`Transfer ownership`}
              disabled={!target}
              title={t`Make ${targetName} the owner?`}
              description={
                <>
                  <p>
                    {actor.role !== "owner"
                      ? t`${targetName} gets the plan, billing and the say over admins, and can delete the workspace. The current owner becomes an admin.`
                      : t`${targetName} gets the plan, billing and the say over admins, and can delete the workspace. You become an admin.`}
                  </p>
                  {subscription ? (
                    <p>
                      {actor.role !== "owner"
                        ? t`The owner’s cards, billing email, address and tax IDs are removed from the workspace, without a refund.`
                        : t`Your cards, billing email, address and tax IDs are removed from the workspace, without a refund.`}{" "}
                      {subscription.current_period_end
                        ? t`The ${planName} plan keeps running until ${formatDay(subscription.current_period_end)}; ${targetName} adds a card under Manage billing to renew it.`
                        : t`The ${planName} plan keeps running until the period ends; ${targetName} adds a card under Manage billing to renew it.`}
                    </p>
                  ) : (
                    plan.data?.billing_enabled && (
                      <p>
                        <Trans>
                          Any card and billing details saved with the workspace are removed.
                        </Trans>
                      </p>
                    )
                  )}
                  {plan.data?.billing_enabled && (
                    <p>
                      <Trans>Past invoices keep the email and card ending that were on them.</Trans>
                    </p>
                  )}
                </>
              }
              phrase={workspace.url_slug}
              confirmLabel={t`Transfer ownership`}
              pendingLabel={t`Transferring…`}
              onConfirm={async () => {
                await unwrap(
                  api.POST("/workspaces/{workspace_id}/transfer", {
                    params: { path: { workspace_id: workspace.id } },
                    body: { user_id: chosen },
                  }),
                );
                setChosen("");
                // Roles, members and the plan all sit under this key.
                await queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
                toast.success(t`${targetName} now owns ${workspace.name}`);
              }}
            />
          </>
        )}
      </SettingsRow>
    </Section>
  );
}
