import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import { type ReactNode, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { invitesQuery, queryKeys } from "@/api/queries";
import type { Client, CreatedInvite } from "@/api/types";
import { CopyField } from "@/components/copy-field";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { errorMessage } from "@/lib/errors";
import { customerInviteUrl } from "@/lib/organizations";
import { formatRelative } from "@/lib/time";

/**
 * Hands a client over: an invite link for the customer. They sign in with
 * Twitch, Kick or YouTube and join as an admin of the workspace (no billing: the
 * organization pays). The link is shown once; making another leaves earlier ones
 * valid until they expire or are revoked.
 */
export function CustomerInviteDialog({
  client,
  children,
}: {
  client: Client;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [created, setCreated] = useState<CreatedInvite | null>(null);
  const queryClient = useQueryClient();
  const invites = useQuery({ ...invitesQuery(client.id), enabled: open });
  const path = { workspace_id: client.id };

  const create = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/invites", {
          params: { path },
          body: { role: "admin", expires_in_days: 7 },
        }),
      ),
    onSuccess: async (invite) => {
      setCreated(invite);
      await queryClient.invalidateQueries({ queryKey: queryKeys.invites(client.id) });
    },
  });

  const revoke = useMutation({
    mutationFn: (inviteId: string) =>
      unwrap(
        api.DELETE("/workspaces/{workspace_id}/invites/{invite_id}", {
          params: { path: { ...path, invite_id: inviteId } },
        }),
      ),
    onSuccess: async (_, inviteId) => {
      if (created?.invite.id === inviteId) setCreated(null);
      await queryClient.invalidateQueries({ queryKey: queryKeys.invites(client.id) });
      toast.success("Invite revoked");
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) setCreated(null);
      }}
    >
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Invite the customer to {client.name}</DialogTitle>
          <DialogDescription>
            They sign in with Twitch, Kick or YouTube and get in as an admin: they run the
            workspace, but never see billing. You keep running it too, and they can still make their
            own workspaces.
          </DialogDescription>
        </DialogHeader>

        {created ? (
          <div className="flex flex-col gap-2 rounded-xl border border-divider bg-sunken p-3.5">
            <p className="text-sm">
              Copy the link now: it won’t be shown again. It works once and expires{" "}
              {formatRelative(created.invite.expires_at)}.
            </p>
            <CopyField
              value={customerInviteUrl(window.location.origin, created.token)}
              label="invite link"
              secret
            />
          </div>
        ) : (
          <Button onClick={() => create.mutate()} disabled={create.isPending}>
            {create.isPending ? "Creating…" : "Create invite link"}
          </Button>
        )}
        {create.error && <p className="text-sm text-destructive">{errorMessage(create.error)}</p>}

        {invites.data && invites.data.length > 0 && (
          <div className="flex flex-col gap-1">
            <span className="text-sm font-semibold">Open invites</span>
            <ul className="flex flex-col">
              {invites.data.map((invite) => (
                <li
                  key={invite.id}
                  className="flex items-center justify-between gap-2 border-t border-divider py-2 text-sm"
                >
                  <span className="text-muted-foreground">
                    Created {formatRelative(invite.created_at)} · expires{" "}
                    {formatRelative(invite.expires_at)}
                  </span>
                  <Button
                    variant="ghost"
                    size="icon"
                    aria-label="Revoke invite"
                    onClick={() => revoke.mutate(invite.id)}
                    disabled={revoke.isPending}
                  >
                    <X />
                  </Button>
                </li>
              ))}
            </ul>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
