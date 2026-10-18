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
import { Badge } from "@/components/ui/badge";
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
  ROLE_DESCRIPTIONS,
  ROLE_LABELS,
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
    ? `${plan.data.usage.members} of ${plan.data.plan.max_members} on ${plan.data.plan.name}`
    : undefined;

  return (
    <Page title="Members & invites" description="Who can see and run this workspace.">
      <Section
        title="Members"
        actions={
          limit && <span className="tabular font-mono text-xs text-muted-foreground">{limit}</span>
        }
      >
        <ul className="flex flex-col">
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
        </ul>
      </Section>

      {can(actor, "account.manage") && <Invites />}

      <Section title="Roles">
        <dl className="grid gap-3 px-5 pb-5 text-sm sm:grid-cols-3">
          {(["operator", "admin", "owner"] as const).map((role) => (
            <div
              key={role}
              className="flex flex-col gap-1 rounded-xl border border-divider bg-sunken p-3.5"
            >
              <dt className="font-semibold">{ROLE_LABELS[role]}</dt>
              <dd className="text-muted-foreground">{ROLE_DESCRIPTIONS[role]}</dd>
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
  const name = member.username ?? "Unknown user";
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
      toast.success(`${name}'s role is now ${ROLE_LABELS[role]}`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const remove = useMutation({
    mutationFn: () =>
      unwrap(api.DELETE("/workspaces/{workspace_id}/members/{user_id}", { params: { path } })),
    onSuccess: async () => {
      await refresh();
      toast.success(`${name} was removed`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <li className="flex flex-wrap items-center gap-3.5 border-t border-divider px-5 py-3">
      <UserAvatar name={member.username} avatarUrl={member.avatar_url} className="size-9" />
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="truncate text-sm font-semibold">
          {name}
          {isYou && <span className="font-normal text-muted-foreground"> (you)</span>}
        </span>
        <span className="text-xs text-muted-foreground">
          {[providerLabel(member.provider), `joined ${formatDate(member.joined_at)}`]
            .filter(Boolean)
            .join(" · ")}
        </span>
      </span>

      {roles.length > 0 ? (
        <Select
          value={member.role}
          onValueChange={(role) => changeRole.mutate(role as WorkspaceRole)}
          disabled={changeRole.isPending}
        >
          <SelectTrigger className="w-36" aria-label={`Role of ${name}`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {[...new Set([member.role, ...roles])].map((role) => (
              <SelectItem key={role} value={role}>
                {ROLE_LABELS[role]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : (
        <Badge
          variant={member.role === "owner" ? "default" : "secondary"}
          className="w-20 justify-center"
        >
          {ROLE_LABELS[member.role]}
        </Badge>
      )}

      {!isYou && canRemove(actor, false, member.role) ? (
        <AlertDialog>
          <AlertDialogTrigger asChild>
            <Button variant="ghost" size="icon" aria-label={`Remove ${name}`}>
              <X />
            </Button>
          </AlertDialogTrigger>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>Remove {name}?</AlertDialogTitle>
              <AlertDialogDescription>
                They lose access to this workspace straight away. You can invite them again later.
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>Cancel</AlertDialogCancel>
              <AlertDialogAction variant="destructive" onClick={() => remove.mutate()}>
                Remove
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      ) : (
        <span className="size-9" />
      )}
    </li>
  );
}

const EXPIRY_OPTIONS = [
  { days: 1, label: "In 1 day" },
  { days: 7, label: "In 7 days" },
  { days: 30, label: "In 30 days" },
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
      toast.success("Invite revoked");
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title="Invite links"
      description="Anyone with a link can join once, with the chosen role, until it expires."
    >
      <div className="flex flex-wrap items-end gap-3 px-5 pb-5">
        <fieldset className="flex flex-col gap-1.5">
          <legend className="pb-1.5 text-sm font-semibold">Role</legend>
          <div className="flex h-10 overflow-hidden rounded-lg border border-input">
            {roles.map((option) => (
              <label
                key={option}
                className="flex cursor-pointer items-center px-4 text-sm font-semibold text-muted-foreground hover:bg-surface-2 has-checked:bg-primary-soft has-checked:text-foreground has-focus-visible:ring-2 has-focus-visible:ring-ring"
              >
                <input
                  type="radio"
                  name="invite-role"
                  value={option}
                  checked={role === option}
                  onChange={() => setRole(option)}
                  className="sr-only"
                />
                {ROLE_LABELS[option]}
              </label>
            ))}
          </div>
        </fieldset>
        <div className="flex flex-col gap-1.5">
          <span className="text-sm font-semibold" id="invite-expiry">
            Expires
          </span>
          <Select value={String(days)} onValueChange={(value) => setDays(Number(value))}>
            <SelectTrigger className="h-10 w-36" aria-labelledby="invite-expiry">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {EXPIRY_OPTIONS.map((option) => (
                <SelectItem key={option.days} value={String(option.days)}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <Button className="h-10" onClick={() => create.mutate()} disabled={create.isPending}>
          <Link2 />
          Create link
        </Button>
      </div>

      {create.error && (
        <p className="mx-5 mb-5 text-sm text-destructive">
          {errorMessage(create.error)}{" "}
          {isApiError(create.error, "plan_limit") && (
            <Link
              to="/$workspace/settings/plan"
              params={{ workspace: workspace.url_slug }}
              className="font-semibold underline"
            >
              See plans
            </Link>
          )}
        </p>
      )}

      {created && <NewInviteLink invite={created} />}

      <ul className="flex flex-col">
        {invites.data?.map((invite) => (
          <li
            key={invite.id}
            className="flex flex-wrap items-center gap-3 border-t border-divider px-5 py-3"
          >
            <Badge variant="secondary">{ROLE_LABELS[invite.role]}</Badge>
            <span className="flex-1 text-sm text-muted-foreground">
              Created {formatRelative(invite.created_at)} · expires{" "}
              {formatRelative(invite.expires_at)}
            </span>
            <Button
              variant="ghost"
              size="sm"
              className="text-destructive-foreground"
              onClick={() => revoke.mutate(invite.id)}
              disabled={revoke.isPending}
            >
              Revoke
            </Button>
          </li>
        ))}
        {invites.data?.length === 0 && !created && (
          <li className="border-t border-divider px-5 py-4 text-sm text-muted-foreground">
            No open invites.
          </li>
        )}
      </ul>
    </Section>
  );
}

/** Shows a new invite's link once; only its hash is stored, so it can't be shown again. */
function NewInviteLink({ invite }: { invite: CreatedInvite }) {
  const url = `${window.location.origin}/invite/${invite.token}`;

  return (
    <div className="mx-5 mb-5 flex flex-col gap-2.5 rounded-xl border border-live/30 bg-live/8 p-3.5">
      <span className="text-sm font-semibold">
        New {ROLE_LABELS[invite.invite.role].toLowerCase()} invite. Copy it now: it won’t be shown
        again.
      </span>
      <CopyField value={url} label="invite link" secret />
    </div>
  );
}
