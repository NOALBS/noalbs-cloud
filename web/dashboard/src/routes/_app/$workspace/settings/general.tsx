import { useMutation, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import { ManagedNotice } from "@/components/orgs/managed-notice";
import { Page, Section } from "@/components/page";
import { SupportAccess } from "@/components/support-access";
import { TransferOwnership } from "@/components/transfer-ownership";
import { TypeToConfirm } from "@/components/type-to-confirm";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useWorkspace } from "@/hooks/use-workspace";
import { errorMessage } from "@/lib/errors";
import { can, ROLE_DESCRIPTIONS, ROLE_LABELS } from "@/lib/permissions";

export const Route = createFileRoute("/_app/$workspace/settings/general")({
  component: GeneralSettings,
});

function GeneralSettings() {
  const { workspace, me, actor } = useWorkspace();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [name, setName] = useState(workspace.name);
  const canManage = can(actor, "account.manage");

  const rename = useMutation({
    mutationFn: () =>
      unwrap(
        api.PATCH("/workspaces/{workspace_id}", {
          params: { path: { workspace_id: workspace.id } },
          body: { name },
        }),
      ),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
      toast.success("Workspace renamed");
    },
  });

  const leave = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE("/workspaces/{workspace_id}/members/{user_id}", {
          params: { path: { workspace_id: workspace.id, user_id: me.id } },
        }),
      ),
    onSuccess: async () => {
      // Navigate away first (`/` reads fresh data), then refresh the caches.
      await navigate({ to: "/" });
      await queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
      await queryClient.invalidateQueries({ queryKey: queryKeys.me });
      toast.success(`You left ${workspace.name}`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    rename.mutate();
  };

  return (
    <Page title="Settings" description="The basics of this workspace.">
      <Section title="Workspace">
        <form onSubmit={submit} className="flex flex-col gap-5 px-5 pb-5">
          <div className="flex max-w-md flex-col gap-2">
            <Label htmlFor="workspace-name">Name</Label>
            <div className="flex gap-2">
              <Input
                id="workspace-name"
                value={name}
                onChange={(event) => setName(event.target.value)}
                maxLength={60}
                disabled={!canManage}
              />
              {canManage && (
                <Button
                  type="submit"
                  disabled={!name.trim() || name.trim() === workspace.name || rename.isPending}
                >
                  Save
                </Button>
              )}
            </div>
            {rename.error && (
              <p className="text-sm text-destructive">{errorMessage(rename.error)}</p>
            )}
          </div>
          <div className="flex flex-col gap-1">
            <span className="text-sm font-semibold">URL</span>
            <span className="font-mono text-sm text-muted-foreground">
              {window.location.host}/{workspace.url_slug}
            </span>
          </div>
        </form>
      </Section>

      <Section title="Your role">
        <div className="flex flex-col gap-1 px-5 pb-5 text-sm">
          <span className="font-semibold">
            {workspace.role
              ? ROLE_LABELS[workspace.role]
              : workspace.organization_role
                ? `Team of ${workspace.managed_by?.name ?? "the organization"}`
                : "Staff (not a member)"}
          </span>
          <span className="text-muted-foreground">
            {workspace.role
              ? ROLE_DESCRIPTIONS[workspace.role]
              : workspace.organization_role
                ? "You run this workspace as part of the organization that owns it."
                : "You're here as NOALBS support, with the workspace's permission or emergency access. Its owners and admins see everything you do here, and its secrets are hidden from you."}
          </span>
        </div>
      </Section>

      <ManagedNotice workspace={workspace} />

      {workspace.role && workspace.role !== "owner" && (
        <Section
          title="Leave workspace"
          description="You’ll lose access until someone invites you again."
          className="border-destructive/40"
        >
          <div className="px-5 pb-5">
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button
                  variant="outline"
                  className="border-destructive/50 text-destructive-foreground"
                >
                  Leave {workspace.name}
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Leave {workspace.name}?</AlertDialogTitle>
                  <AlertDialogDescription>
                    You’ll lose access to its switchers straight away.
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Stay</AlertDialogCancel>
                  <AlertDialogAction variant="destructive" onClick={() => leave.mutate()}>
                    Leave
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          </div>
        </Section>
      )}

      {can(actor, "workspace.support_access") && (
        <div id="support-access" className="scroll-mt-20">
          <SupportAccess />
        </div>
      )}

      {can(actor, "workspace.transfer") && <TransferOwnership />}

      {can(actor, "workspace.delete") && (
        <Section
          title="Delete workspace"
          description="Deletes its switchers, stream history, members and invites. This can’t be undone."
          className="border-destructive/40"
        >
          <div className="px-5 pb-5">
            <TypeToConfirm
              trigger={`Delete ${workspace.name}`}
              title={`Delete ${workspace.name}?`}
              description={
                <>
                  <p>
                    Everything in it is deleted for good: switchers, stream history, members and
                    invites. OBS browser sources stop working.
                  </p>
                  <p>A subscription ends straight away, without a refund.</p>
                </>
              }
              phrase={workspace.url_slug}
              confirmLabel="Delete workspace"
              onConfirm={async () => {
                await unwrap(
                  api.DELETE("/workspaces/{workspace_id}", {
                    params: { path: { workspace_id: workspace.id } },
                  }),
                );
                // Navigate away first (`/` reads fresh data), then refresh the caches.
                await navigate({ to: "/" });
                await queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
                await queryClient.invalidateQueries({ queryKey: queryKeys.me });
                toast.success(`${workspace.name} deleted`);
              }}
            />
          </div>
        </Section>
      )}
    </Page>
  );
}
