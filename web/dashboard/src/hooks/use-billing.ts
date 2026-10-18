import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import type { BillingInterval, WorkspacePlan } from "@/api/types";
import { errorMessage } from "@/lib/errors";
import { useWorkspace } from "./use-workspace";

type CheckoutBody = {
  plan_code?: string | null;
  interval: BillingInterval;
  addons?: { code: string; quantity: number }[];
};

/** Starts a subscription with Stripe Checkout, or opens the billing portal. */
export function useStripeRedirect() {
  const { workspace } = useWorkspace();
  const path = { params: { path: { workspace_id: workspace.id } } };
  return useMutation({
    mutationFn: (body: CheckoutBody | "portal") =>
      body === "portal"
        ? unwrap(api.POST("/workspaces/{workspace_id}/billing/portal", path))
        : unwrap(api.POST("/workspaces/{workspace_id}/billing/checkout", { ...path, body })),
    onSuccess: ({ url }) => window.location.assign(url),
    onError: (error) => toast.error(errorMessage(error)),
  });
}

export type BillingChange =
  | { kind: "plan"; plan_code: string }
  | { kind: "addon"; code: string; quantity: number }
  | { kind: "interval"; interval: BillingInterval }
  | { kind: "resume" };

/**
 * Changes the existing subscription (add-ons, plan, billing period, resuming).
 * The API answers with the workspace's plan as it is now.
 */
export function useBillingChange(successMessage?: string) {
  const { workspace } = useWorkspace();
  const queryClient = useQueryClient();
  const path = { workspace_id: workspace.id };

  return useMutation({
    mutationFn: (change: BillingChange): Promise<WorkspacePlan> => {
      switch (change.kind) {
        case "plan":
          return unwrap(
            api.PUT("/workspaces/{workspace_id}/billing/plan", {
              params: { path },
              body: { plan_code: change.plan_code },
            }),
          );
        case "addon":
          return unwrap(
            api.PUT("/workspaces/{workspace_id}/billing/addons/{addon_code}", {
              params: { path: { ...path, addon_code: change.code } },
              body: { quantity: change.quantity },
            }),
          );
        case "interval":
          return unwrap(
            api.PUT("/workspaces/{workspace_id}/billing/interval", {
              params: { path },
              body: { interval: change.interval },
            }),
          );
        case "resume":
          return unwrap(
            api.POST("/workspaces/{workspace_id}/billing/resume", { params: { path } }),
          );
      }
    },
    onSuccess: async (plan) => {
      queryClient.setQueryData(queryKeys.workspacePlan(workspace.id), plan);
      // A plan change can pause or resume switchers, and shows in the workspace list.
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.switchers(workspace.id) }),
        queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
      ]);
      if (successMessage) toast.success(successMessage);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
}
