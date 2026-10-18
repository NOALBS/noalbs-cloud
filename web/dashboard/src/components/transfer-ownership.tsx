import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { membersQuery, queryKeys, workspacePlanQuery } from "@/api/queries";
import { Section } from "@/components/page";
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
import { ROLE_LABELS, transferCandidates } from "@/lib/permissions";

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
  const targetName = target?.username ?? "this member";
  const subscription = plan.data?.subscription;
  const self = actor.role !== "owner" ? "The current owner" : "You";

  return (
    <Section
      title="Transfer ownership"
      description={
        actor.role !== "owner"
          ? "Make another member the owner. The current owner stays as an admin."
          : "Make an admin the owner, for example so you can leave. You stay as an admin."
      }
    >
      <div className="flex flex-wrap items-center gap-2 px-5 pb-5">
        {members.data && candidates.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            Only an admin can become the owner.{" "}
            <Link
              to="/$workspace/settings/members"
              params={{ workspace: workspace.url_slug }}
              className="font-medium text-primary hover:underline"
            >
              Make someone an admin
            </Link>{" "}
            first.
          </p>
        ) : (
          <>
            <Select value={chosen} onValueChange={setChosen}>
              <SelectTrigger className="w-56" aria-label="New owner">
                <SelectValue placeholder="Choose the new owner" />
              </SelectTrigger>
              <SelectContent>
                {candidates.map((member) => (
                  <SelectItem key={member.user_id} value={member.user_id}>
                    {member.username ?? "Unnamed"}
                    {actor.staff && ` (${ROLE_LABELS[member.role].toLowerCase()})`}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <TypeToConfirm
              trigger="Transfer ownership"
              disabled={!target}
              title={`Make ${targetName} the owner?`}
              description={
                <>
                  <p>
                    {targetName} gets the plan, billing and the say over admins, and can delete the
                    workspace. {self} become{actor.staff ? "s" : ""} an admin.
                  </p>
                  {subscription ? (
                    <p>
                      {actor.role !== "owner" ? "The owner’s" : "Your"} cards, billing email,
                      address and tax IDs are removed from the workspace, without a refund. The{" "}
                      {plan.data?.plan.name} plan keeps running
                      {subscription.current_period_end
                        ? ` until ${formatDay(subscription.current_period_end)}`
                        : " until the period ends"}
                      ; {targetName} adds a card under Manage billing to renew it.
                    </p>
                  ) : (
                    plan.data?.billing_enabled && (
                      <p>Any card and billing details saved with the workspace are removed.</p>
                    )
                  )}
                  {plan.data?.billing_enabled && (
                    <p>Past invoices keep the email and card ending that were on them.</p>
                  )}
                </>
              }
              phrase={workspace.url_slug}
              confirmLabel="Transfer ownership"
              pendingLabel="Transferring…"
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
                toast.success(`${targetName} now owns ${workspace.name}`);
              }}
            />
          </>
        )}
      </div>
    </Section>
  );
}
