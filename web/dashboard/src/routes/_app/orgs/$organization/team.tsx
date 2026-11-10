import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { Link2, X } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys, teamInvitesQuery, teamQuery } from "@/api/queries";
import type { CreatedTeamInvite, OrganizationRole, TeamMember } from "@/api/types";
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
import { useOrganization } from "@/hooks/use-organization";
import { errorMessage } from "@/lib/errors";
import {
  organizationRoleDescriptions,
  organizationRoleLabels,
  teamInviteUrl,
} from "@/lib/organizations";
import { formatDate, formatRelative } from "@/lib/time";

export const Route = createFileRoute("/_app/orgs/$organization/team")({
  component: TeamPage,
});

const ROLES: OrganizationRole[] = ["member", "admin"];

function TeamPage() {
  const { organization, me, isAdmin } = useOrganization();
  const team = useQuery(teamQuery(organization.id));

  return (
    <Page title={t`Team`} description={t`Who can see and run your clients.`}>
      <Section title={t`Members`} description={t`${organization.usage.team} on the team`}>
        {!team.data ? (
          <Skeleton className="mx-5 mb-5 h-24" />
        ) : (
          <RowList>
            {team.data.map((member) => (
              <MemberRow
                key={member.user_id}
                member={member}
                organizationId={organization.id}
                isYou={member.user_id === me.id}
                isAdmin={isAdmin}
              />
            ))}
          </RowList>
        )}
      </Section>

      {isAdmin && <Invites />}

      <Section title={t`Roles`}>
        <dl className="flex flex-col text-sm">
          {ROLES.map((role) => (
            <div
              key={role}
              className="flex flex-col gap-1 border-t border-divider px-5 py-3 sm:flex-row sm:gap-3"
            >
              <dt className="w-20 shrink-0 font-semibold">{organizationRoleLabels()[role]}</dt>
              <dd className="text-muted-foreground">{organizationRoleDescriptions()[role]}</dd>
            </div>
          ))}
        </dl>
      </Section>
    </Page>
  );
}

function MemberRow({
  member,
  organizationId,
  isYou,
  isAdmin,
}: {
  member: TeamMember;
  organizationId: string;
  isYou: boolean;
  isAdmin: boolean;
}) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const name = member.username ?? t`Unknown user`;
  const path = { organization_id: organizationId, user_id: member.user_id };

  const refresh = () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.team(organizationId) }),
      queryClient.invalidateQueries({ queryKey: queryKeys.organizations }),
    ]);

  const changeRole = useMutation({
    mutationFn: (role: OrganizationRole) =>
      unwrap(
        api.PATCH("/organizations/{organization_id}/members/{user_id}", {
          params: { path },
          body: { role },
        }),
      ),
    onSuccess: async (_, role) => {
      await refresh();
      toast.success(t`${name}’s role is now ${organizationRoleLabels()[role]}`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const remove = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE("/organizations/{organization_id}/members/{user_id}", { params: { path } }),
      ),
    onSuccess: async () => {
      await refresh();
      if (isYou) {
        await navigate({ to: "/" });
        return;
      }
      toast.success(t`${name} was removed`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Row
      lead={<UserAvatar name={member.username} avatarUrl={member.avatar_url} className="size-9" />}
      title={
        <span>
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
      {isAdmin ? (
        <Select
          value={member.role}
          onValueChange={(role) => changeRole.mutate(role as OrganizationRole)}
          disabled={changeRole.isPending}
        >
          <SelectTrigger className="w-36" aria-label={t`Role of ${name}`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {ROLES.map((role) => (
              <SelectItem key={role} value={role}>
                {organizationRoleLabels()[role]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : (
        <span className="text-sm text-muted-foreground">
          {organizationRoleLabels()[member.role]}
        </span>
      )}

      {isAdmin || isYou ? (
        <AlertDialog>
          <AlertDialogTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              aria-label={isYou ? t`Leave the team` : t`Remove ${name}`}
            >
              <X />
            </Button>
          </AlertDialogTrigger>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>{isYou ? t`Leave the team?` : t`Remove ${name}?`}</AlertDialogTitle>
              <AlertDialogDescription>
                {isYou
                  ? t`You lose access to the organization and all its clients straight away. An admin can invite you again later. The last admin can’t go.`
                  : t`They lose access to the organization and all its clients straight away. You can invite them again later. The last admin can’t go.`}
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>
                <Trans>Cancel</Trans>
              </AlertDialogCancel>
              <AlertDialogAction variant="destructive" onClick={() => remove.mutate()}>
                {isYou ? t`Leave` : t`Remove`}
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

function Invites() {
  const { organization } = useOrganization();
  const queryClient = useQueryClient();
  const invites = useQuery(teamInvitesQuery(organization.id));
  const [role, setRole] = useState<OrganizationRole>("member");
  const [created, setCreated] = useState<CreatedTeamInvite | null>(null);
  const path = { organization_id: organization.id };

  const create = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/organizations/{organization_id}/invites", {
          params: { path },
          body: { role, expires_in_days: 7 },
        }),
      ),
    onSuccess: async (invite) => {
      setCreated(invite);
      await queryClient.invalidateQueries({ queryKey: queryKeys.teamInvites(organization.id) });
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const revoke = useMutation({
    mutationFn: (inviteId: string) =>
      unwrap(
        api.DELETE("/organizations/{organization_id}/invites/{invite_id}", {
          params: { path: { ...path, invite_id: inviteId } },
        }),
      ),
    onSuccess: async (_, inviteId) => {
      if (created?.invite.id === inviteId) setCreated(null);
      await queryClient.invalidateQueries({ queryKey: queryKeys.teamInvites(organization.id) });
      toast.success(t`Invite revoked`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title={t`Invite links`}
      description={t`Someone who opens a link signs in and joins your team. A link works once and expires after 7 days.`}
    >
      <RowList>
        <Row title={t`New invite`}>
          <SegmentedControl
            label={t`Role for the invite`}
            value={role}
            onValueChange={setRole}
            options={ROLES.map((value) => ({ value, label: organizationRoleLabels()[value] }))}
          />
          <Button onClick={() => create.mutate()} disabled={create.isPending}>
            <Link2 />
            <Trans>Create invite link</Trans>
          </Button>
        </Row>
      </RowList>

      {created && (
        <div className="flex flex-col gap-2 border-t border-divider px-5 py-3">
          <p className="text-sm">
            <Trans>
              New invite ({organizationRoleLabels()[created.invite.role]}). Copy it now: it won’t be
              shown again.
            </Trans>
          </p>
          <CopyField
            value={teamInviteUrl(window.location.origin, created.token)}
            label={t`invite link`}
            secret
          />
        </div>
      )}

      <RowList>
        {invites.data?.map((invite) => (
          <Row
            key={invite.id}
            compactAction
            title={t`${organizationRoleLabels()[invite.role]} invite`}
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
              aria-label={t`Revoke invite`}
              onClick={() => revoke.mutate(invite.id)}
              disabled={revoke.isPending}
            >
              <Trans>Revoke</Trans>
            </Button>
          </Row>
        ))}
        {invites.data?.length === 0 && !created && (
          <li className="border-t border-divider px-5 py-3 text-sm text-muted-foreground">
            <Trans>No open invites.</Trans>
          </li>
        )}
      </RowList>
    </Section>
  );
}
