import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { Link2, X } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { invitesQuery, membersQuery, queryKeys, workspacePlanQuery } from "@/api/queries";
import type { CreatedInvite, Member, WorkspaceRole } from "@/api/types";
import { CopyField } from "@/components/copy-field";
import { Page, Section } from "@/components/page";
import { Row, RowList } from "@/components/row-list";
import { SegmentedControl } from "@/components/segmented-control";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { providerLabel, UserAvatar } from "@/components/user-avatar";
import { useWorkspace } from "@/hooks/use-workspace";
import { errorMessage, isApiError } from "@/lib/errors";
import {
  type Actor,
  assignableRoles,
  can,
  canRemove,
  invitableRoles,
  roleDescriptions,
  roleLabels,
} from "@/lib/permissions";
import { formatDate, formatRelative } from "@/lib/time";

export const Route = createFileRoute("/_app/$workspace/settings/members")({
  component: MembersPage,
});

function MembersPage() {
  const { workspace, me, actor } = useWorkspace();
  const members = useQuery(membersQuery(workspace.id));
  const plan = useQuery(workspacePlanQuery(workspace.id));

  const limit = plan.data
    ? t`${plan.data.usage.members} of ${plan.data.plan.max_members} on ${plan.data.plan.name}`
    : undefined;

  return (
    <Page title={t`Members & invites`} description={t`Who can see and run this workspace.`}>
      <Section
        title={t`Members`}
        actions={
          limit && (
            <span className="tabular font-display text-xs text-muted-foreground">{limit}</span>
          )
        }
      >
        <RowList>
          {members.isPending &&
            [0, 1, 2].map((index) => (
              <li
                key={index}
                className="flex items-center gap-3.5 border-t border-divider px-5 py-3"
              >
                <Skeleton className="size-9 rounded-full" />
                <Skeleton className="h-4 w-40" />
              </li>
            ))}
          {members.data?.map((member) => (
            <MemberRow
              key={member.user_id}
              member={member}
              actor={actor}
              isYou={member.user_id === me.id}
              workspaceId={workspace.id}
            />
          ))}
        </RowList>
      </Section>

      {can(actor, "account.manage") && <Invites />}

      <Section title={t`Roles`}>
        <dl className="text-sm">
          {(["operator", "admin", "owner"] as const).map((role) => (
            <div
              key={role}
              className="flex flex-col gap-1 border-t border-divider px-5 py-3 sm:flex-row sm:gap-6"
            >
              <dt className="w-24 shrink-0 font-semibold">{roleLabels()[role]}</dt>
              <dd className="text-xs text-muted-foreground">{roleDescriptions()[role]}</dd>
            </div>
          ))}
        </dl>
      </Section>
    </Page>
  );
}

function MemberRow({
  member,
  actor,
  isYou,
  workspaceId,
}: {
  member: Member;
  actor: Actor;
  isYou: boolean;
  workspaceId: string;
}) {
  const queryClient = useQueryClient();
  const name = member.username ?? t`Unknown user`;
  const roles = isYou ? [] : assignableRoles(actor, member.role);
  const path = { workspace_id: workspaceId, user_id: member.user_id };

  const refresh = () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.members(workspaceId) }),
      queryClient.invalidateQueries({ queryKey: queryKeys.workspacePlan(workspaceId) }),
    ]);

  const changeRole = useMutation({
    mutationFn: (role: WorkspaceRole) =>
      unwrap(
        api.PATCH("/workspaces/{workspace_id}/members/{user_id}", {
          params: { path },
          body: { role },
        }),
      ),
    onSuccess: async (_, role) => {
      await refresh();
      toast.success(t`${name}'s role is now ${roleLabels()[role]}`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const remove = useMutation({
    mutationFn: () =>
      unwrap(api.DELETE("/workspaces/{workspace_id}/members/{user_id}", { params: { path } })),
    onSuccess: async () => {
      await refresh();
      toast.success(t`${name} was removed`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Row
      lead={<UserAvatar name={member.username} avatarUrl={member.avatar_url} className="size-9" />}
      title={
        <span className="truncate text-sm font-semibold">
          {name}
          {isYou && (
            <span className="font-normal text-muted-foreground">
              {" "}
              <Trans>(you)</Trans>
            </span>
          )}
        </span>
      }
      meta={[providerLabel(member.provider), t`joined ${formatDate(member.joined_at)}`]
        .filter(Boolean)
        .join(" · ")}
    >
      {roles.length > 0 ? (
        <Select
          value={member.role}
          onValueChange={(role) => changeRole.mutate(role as WorkspaceRole)}
          disabled={changeRole.isPending}
        >
          <SelectTrigger className="w-36" aria-label={t`Role of ${name}`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {[...new Set([member.role, ...roles])].map((role) => (
              <SelectItem key={role} value={role}>
                {roleLabels()[role]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : (
        <span className="w-36 text-sm text-muted-foreground">{roleLabels()[member.role]}</span>
      )}

      {!isYou && canRemove(actor, false, member.role) ? (
        <AlertDialog>
          <AlertDialogTrigger asChild>
            <Button variant="ghost" size="icon" aria-label={t`Remove ${name}`}>
              <X />
            </Button>
          </AlertDialogTrigger>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>
                <Trans>Remove {name}</Trans>?
              </AlertDialogTitle>
              <AlertDialogDescription>
                <Trans>
                  They lose access to this workspace straight away. You can invite them again later.
                </Trans>
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>
                <Trans>Cancel</Trans>
              </AlertDialogCancel>
              <AlertDialogAction variant="destructive" onClick={() => remove.mutate()}>
                <Trans>Remove</Trans>
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      ) : (
        <span className="size-9" />
      )}
    </Row>
  );
}

const expiryOptions = () => [
  { days: 1, label: t`In 1 day` },
  { days: 7, label: t`In 7 days` },
  { days: 30, label: t`In 30 days` },
];

function Invites() {
  const { workspace, actor } = useWorkspace();
  const queryClient = useQueryClient();
  const invites = useQuery(invitesQuery(workspace.id));
  const roles = invitableRoles(actor);
  const [role, setRole] = useState<WorkspaceRole>("operator");
  const [days, setDays] = useState(7);
  const [created, setCreated] = useState<CreatedInvite | null>(null);
  const path = { workspace_id: workspace.id };

  const create = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/invites", {
          params: { path },
          body: { role, expires_in_days: days },
        }),
      ),
    onSuccess: async (invite) => {
      setCreated(invite);
      await queryClient.invalidateQueries({ queryKey: queryKeys.invites(workspace.id) });
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
      await queryClient.invalidateQueries({ queryKey: queryKeys.invites(workspace.id) });
      toast.success(t`Invite revoked`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title={t`Invites`}
      description={t`Anyone with a link can join once, with the chosen role, until it expires.`}
    >
      <RowList>
        <Row title={t`New invite`}>
          <SegmentedControl
            label={t`Role`}
            value={role}
            onValueChange={setRole}
            options={roles.map((option) => ({ value: option, label: roleLabels()[option] }))}
          />
          <div className="flex flex-col gap-1.5">
            <span className="sr-only" id="invite-expiry">
              <Trans>Expires</Trans>
            </span>
            <Select value={String(days)} onValueChange={(value) => setDays(Number(value))}>
              <SelectTrigger className="h-10 w-36" aria-labelledby="invite-expiry">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {expiryOptions().map((option) => (
                  <SelectItem key={option.days} value={String(option.days)}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <Button className="h-10" onClick={() => create.mutate()} disabled={create.isPending}>
            <Link2 />
            <Trans>Create link</Trans>
          </Button>
        </Row>
      </RowList>

      {create.error && (
        <p className="mx-5 mb-5 text-sm text-destructive">
          {errorMessage(create.error)}{" "}
          {isApiError(create.error, "plan_limit") && (
            <Link
              to="/$workspace/settings/plan"
              params={{ workspace: workspace.url_slug }}
              className="font-semibold underline"
            >
              <Trans>See plans</Trans>
            </Link>
          )}
        </p>
      )}

      {created && <NewInviteLink invite={created} />}

      <RowList>
        {invites.data?.map((invite) => (
          <Row
            key={invite.id}
            compactAction
            title={t`${roleLabels()[invite.role]} invite`}
            meta={
              <Trans>
                Created {formatRelative(invite.created_at)} · expires{" "}
                {formatRelative(invite.expires_at)}
              </Trans>
            }
          >
            <Button
              variant="ghost"
              size="sm"
              className="text-destructive-foreground"
              onClick={() => revoke.mutate(invite.id)}
              disabled={revoke.isPending}
            >
              <Trans>Revoke</Trans>
            </Button>
          </Row>
        ))}
        {invites.data?.length === 0 && !created && (
          <li className="border-t border-divider px-5 py-4 text-sm text-muted-foreground">
            <Trans>No open invites.</Trans>
          </li>
        )}
      </RowList>
    </Section>
  );
}

/** Shows a new invite's link once; only its hash is stored, so it can't be shown again. */
function NewInviteLink({ invite }: { invite: CreatedInvite }) {
  const url = `${window.location.origin}/invite/${invite.token}`;

  return (
    <div className="flex flex-col gap-2.5 border-t border-divider px-5 py-3">
      <span className="text-sm font-semibold">
        <Trans>
          New invite ({roleLabels()[invite.invite.role]}). Copy it now: it won’t be shown again.
        </Trans>
      </span>
      <CopyField value={url} label={t`invite link`} secret />
    </div>
  );
}
