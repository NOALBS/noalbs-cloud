import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import { ChangeWorkspaceUrl } from "@/components/change-workspace-url";
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
import { can, roleDescriptions, roleLabels } from "@/lib/permissions";

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
      toast.success(t`Workspace renamed`);
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
      toast.success(t`You left ${workspace.name}`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    rename.mutate();
  };

  return (
    <Page title={t`Settings`} description={t`The basics of this workspace.`}>
      <Section title={t`Workspace`}>
        <div className="flex flex-col gap-5 px-5 pb-5">
          <form onSubmit={submit} className="flex max-w-md flex-col gap-2">
            <Label htmlFor="workspace-name">
              <Trans>Name</Trans>
            </Label>
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
                  <Trans>Save</Trans>
                </Button>
              )}
            </div>
            {rename.error && (
              <p className="text-sm text-destructive">{errorMessage(rename.error)}</p>
            )}
          </form>
          <div className="flex flex-col gap-1">
            <span className="text-sm font-semibold">
              <Trans>URL</Trans>
            </span>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
              <span className="font-mono text-sm break-all text-muted-foreground">
                {window.location.host}/{workspace.url_slug}
              </span>
              {canManage && <ChangeWorkspaceUrl />}
            </div>
          </div>
        </div>
      </Section>

      <Section title={t`Your role`}>
        <div className="flex flex-col gap-1 px-5 pb-5 text-sm">
          <span className="font-semibold">
            {workspace.role
              ? roleLabels()[workspace.role]
              : workspace.organization_role
                ? t`Team of ${workspace.managed_by?.name ?? "the organization"}`
                : t`Staff (not a member)`}
          </span>
          <span className="text-muted-foreground">
            {workspace.role
              ? roleDescriptions()[workspace.role]
              : workspace.organization_role
                ? t`You run this workspace as part of the organization that owns it.`
                : t`You're here as NOALBS support, with the workspace's permission or emergency access. Its owners and admins see everything you do here, and its secrets are hidden from you.`}
          </span>
        </div>
      </Section>

      <ManagedNotice workspace={workspace} />

      {workspace.role && workspace.role !== "owner" && (
        <Section
          title={t`Leave workspace`}
          description={t`You’ll lose access until someone invites you again.`}
          className="border-destructive/40"
        >
          <div className="px-5 pb-5">
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button
                  variant="outline"
                  className="border-destructive/50 text-destructive-foreground"
                >
                  <Trans>Leave {workspace.name}</Trans>
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>
                    <Trans>Leave {workspace.name}</Trans>?
                  </AlertDialogTitle>
                  <AlertDialogDescription>
                    <Trans>You’ll lose access to its switchers straight away.</Trans>
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>
                    <Trans>Stay</Trans>
                  </AlertDialogCancel>
                  <AlertDialogAction variant="destructive" onClick={() => leave.mutate()}>
                    <Trans>Leave</Trans>
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
          title={t`Delete workspace`}
          description={t`Deletes its switchers, stream history, members and invites. This can’t be undone.`}
          className="border-destructive/40"
        >
          <div className="px-5 pb-5">
            <TypeToConfirm
              trigger={t`Delete ${workspace.name}`}
              title={t`Delete ${workspace.name}?`}
              description={
                <>
                  <p>
                    <Trans>
                      Everything in it is deleted for good: switchers, stream history, members and
                      invites. OBS browser sources stop working.
                    </Trans>
                  </p>
                  <p>
                    <Trans>A subscription ends straight away, without a refund.</Trans>
                  </p>
                </>
              }
              phrase={workspace.url_slug}
              confirmLabel={t`Delete workspace`}
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
                toast.success(t`${workspace.name} deleted`);
              }}
            />
          </div>
        </Section>
      )}
    </Page>
  );
}
