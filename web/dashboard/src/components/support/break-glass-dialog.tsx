import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { type ReactNode, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import type { SupportLevel } from "@/api/types";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { errorMessage } from "@/lib/errors";
import { supportLevelDescriptions, supportLevelLabels } from "@/lib/support";

const MIN_REASON = 10;

/**
 * Staff: open a workspace without its permission for one hour, to look or to act
 * on its switchers. The level and reason go to its owners and admins.
 */
export function BreakGlassDialog({
  workspaceId,
  workspaceName,
  children,
}: {
  workspaceId: string;
  workspaceName: string;
  children: ReactNode;
}) {
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [reason, setReason] = useState("");
  // Least first: control has to be picked on purpose.
  const [level, setLevel] = useState<SupportLevel>("view");

  const breakGlass = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/admin/workspaces/{workspace_id}/break-glass", {
          params: { path: { workspace_id: workspaceId } },
          body: { reason: reason.trim(), level },
        }),
      ),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
        queryClient.invalidateQueries({ queryKey: queryKeys.adminWorkspace(workspaceId) }),
      ]);
      setOpen(false);
      setReason("");
      setLevel("view");
      toast.success(t`Emergency access to ${workspaceName} for one hour`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            <Trans>Emergency access to {workspaceName}</Trans>?
          </DialogTitle>
          <DialogDescription>
            <Trans>
              Only for abuse, a legal obligation or a security problem, for one hour. Members,
              billing and secrets stay off-limits. Its owners and admins see your name, the level
              and this reason until they mark it as seen, and everything you do is in their log.
            </Trans>
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-2">
          <Label htmlFor="break-glass-level">
            <Trans>Access</Trans>
          </Label>
          <Select value={level} onValueChange={(next) => setLevel(next as SupportLevel)}>
            <SelectTrigger id="break-glass-level" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(["view", "control"] as const).map((option) => (
                <SelectItem key={option} value={option}>
                  {supportLevelLabels()[option]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <p className="text-xs text-muted-foreground">{supportLevelDescriptions()[level]}</p>
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="break-glass-reason">
            <Trans>Reason</Trans>
          </Label>
          <textarea
            id="break-glass-reason"
            value={reason}
            onChange={(event) => setReason(event.target.value)}
            maxLength={500}
            rows={3}
            className="w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
          />
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)}>
            <Trans>Cancel</Trans>
          </Button>
          <Button
            variant="destructive"
            onClick={() => breakGlass.mutate()}
            disabled={reason.trim().length < MIN_REASON || breakGlass.isPending}
          >
            <Trans>Open for one hour</Trans>
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
