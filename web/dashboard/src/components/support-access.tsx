import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { supportAccessQuery, supportLogQuery } from "@/api/queries";
import type { EmergencyAccess, SupportLevel, SupportLogEntry } from "@/api/types";
import { Section } from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useWorkspace } from "@/hooks/use-workspace";
import { errorMessage } from "@/lib/errors";
import { currentLocale } from "@/lib/locale";
import { can, isSupport } from "@/lib/permissions";
import {
  describeSupportAccess,
  isSupportChange,
  SUPPORT_DURATIONS,
  SUPPORT_LEVEL_DESCRIPTIONS,
  SUPPORT_LEVEL_LABELS,
  supportAccessBasis,
} from "@/lib/support";
import { formatRelative } from "@/lib/time";

/**
 * Workspace settings (owners and admins): letting NOALBS support in for a while,
 * emergency access staff opened without permission, and everything staff did here.
 */
export function SupportAccess() {
  const { workspace, actor } = useWorkspace();
  const canGrant = can(actor, "workspace.support_access") && !isSupport(actor);
  const access = useQuery(supportAccessQuery(workspace.id));
  const grant = access.data?.grant ?? null;
  const [editing, setEditing] = useState(false);

  return (
    <Section
      title="NOALBS support access"
      description="Let NOALBS support into this workspace while they help you. They see your stream servers’ stats URLs, to help fix them, but never passwords or raw stats, and everything they do here is listed below."
    >
      <div className="flex flex-col gap-5 px-5 pb-5">
        {grant ? (
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border bg-surface-2 px-4 py-3">
            <div className="flex flex-col gap-0.5 text-sm">
              <span className="font-semibold">
                {SUPPORT_LEVEL_LABELS[grant.level]} until{" "}
                {new Date(grant.expires_at).toLocaleString(currentLocale())}
              </span>
              <span className="text-muted-foreground">
                Allowed {formatRelative(grant.created_at)}
                {grant.granted_by && ` by ${grant.granted_by}`}.{" "}
                {SUPPORT_LEVEL_DESCRIPTIONS[grant.level]}
              </span>
            </div>
            {canGrant && !editing && (
              <div className="flex gap-2">
                <Button variant="outline" onClick={() => setEditing(true)}>
                  Change
                </Button>
                <EndButton workspaceId={workspace.id} />
              </div>
            )}
          </div>
        ) : (
          access.isSuccess && (
            <p className="text-sm text-muted-foreground">
              NOALBS support can’t open this workspace right now. Only in an emergency (abuse, a
              legal obligation or a security problem) can staff look without your permission, for an
              hour, and you’ll see who and why here.
            </p>
          )
        )}

        {access.data && access.data.emergency.length > 0 && (
          <EmergencyList workspaceId={workspace.id} emergency={access.data.emergency} />
        )}

        {canGrant && (editing || (access.isSuccess && !grant)) && (
          <GrantForm
            workspaceId={workspace.id}
            initialLevel={grant?.level ?? "view"}
            onDone={() => setEditing(false)}
            onCancel={grant ? () => setEditing(false) : undefined}
          />
        )}

        <AccessLog workspaceId={workspace.id} />
      </div>
    </Section>
  );
}

function GrantForm({
  workspaceId,
  initialLevel,
  onDone,
  onCancel,
}: {
  workspaceId: string;
  initialLevel: SupportLevel;
  onDone: () => void;
  onCancel?: () => void;
}) {
  const queryClient = useQueryClient();
  const [level, setLevel] = useState<SupportLevel>(initialLevel);
  const [hours, setHours] = useState<number>(SUPPORT_DURATIONS[0].hours);

  const grant = useMutation({
    mutationFn: () =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/support-access", {
          params: { path: { workspace_id: workspaceId } },
          body: { level, hours },
        }),
      ),
    onSuccess: (data) => {
      queryClient.setQueryData(supportAccessQuery(workspaceId).queryKey, data);
      onDone();
      toast.success("NOALBS support can come in now");
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <Select value={level} onValueChange={(next) => setLevel(next as SupportLevel)}>
          <SelectTrigger className="w-48" aria-label="What support may do">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {(["view", "control"] as const).map((option) => (
              <SelectItem key={option} value={option}>
                {SUPPORT_LEVEL_LABELS[option]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select value={String(hours)} onValueChange={(next) => setHours(Number(next))}>
          <SelectTrigger className="w-32" aria-label="For how long">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SUPPORT_DURATIONS.map((duration) => (
              <SelectItem key={duration.hours} value={String(duration.hours)}>
                {duration.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button onClick={() => grant.mutate()} disabled={grant.isPending}>
          Allow support access
        </Button>
        {onCancel && (
          <Button variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
        )}
      </div>
      <p className="text-sm text-muted-foreground">{SUPPORT_LEVEL_DESCRIPTIONS[level]}</p>
    </div>
  );
}

function EndButton({ workspaceId }: { workspaceId: string }) {
  const queryClient = useQueryClient();
  const end = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE("/workspaces/{workspace_id}/support-access", {
          params: { path: { workspace_id: workspaceId } },
        }),
      ),
    onSuccess: () => {
      queryClient.setQueryData(supportAccessQuery(workspaceId).queryKey, (current) =>
        current ? { ...current, grant: null } : current,
      );
      toast.success("Support access ended");
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Button
      variant="outline"
      className="border-destructive/50 text-destructive-foreground"
      onClick={() => end.mutate()}
      disabled={end.isPending}
    >
      End now
    </Button>
  );
}

function AccessLog({ workspaceId }: { workspaceId: string }) {
  const log = useInfiniteQuery(supportLogQuery(workspaceId));
  const entries = log.data?.pages.flat() ?? [];

  return (
    <div className="flex flex-col gap-2">
      <h3 className="text-sm font-semibold">What staff did here</h3>
      {log.isError && <p className="text-sm text-destructive">{errorMessage(log.error)}</p>}
      {log.isSuccess && entries.length === 0 && (
        <p className="text-sm text-muted-foreground">Nothing: NOALBS staff haven’t been here.</p>
      )}
      {entries.length > 0 && (
        <ul className="flex flex-col divide-y divide-divider rounded-xl border">
          {entries.map((entry) => (
            <LogRow key={entry.id} entry={entry} />
          ))}
        </ul>
      )}
      {log.hasNextPage && (
        <Button
          variant="ghost"
          className="self-start"
          onClick={() => log.fetchNextPage()}
          disabled={log.isFetchingNextPage}
        >
          Show older
        </Button>
      )}
      <p className="text-xs text-muted-foreground">
        Looking around is listed once an hour per page, every change each time. Kept for a year.
      </p>
    </div>
  );
}

function LogRow({ entry }: { entry: SupportLogEntry }) {
  const change = isSupportChange(entry);
  const basis = supportAccessBasis(entry);
  return (
    <li className="flex flex-wrap items-center justify-between gap-x-4 gap-y-1 px-4 py-2.5 text-sm">
      <span className="flex min-w-0 flex-col">
        <span className={change ? "font-semibold" : undefined}>
          {describeSupportAccess(entry)}
          {entry.switcher_name && (
            <span className="font-normal text-muted-foreground"> · {entry.switcher_name}</span>
          )}
        </span>
        <span className="text-xs text-muted-foreground">
          {entry.staff_name} · {formatRelative(entry.created_at)} ·{" "}
          {new Date(entry.created_at).toLocaleString(currentLocale())}
        </span>
      </span>
      <Badge
        variant={basis.tone === "granted" ? "secondary" : "outline"}
        className={basis.tone === "emergency" ? "border-low/50 text-low" : undefined}
      >
        {basis.label}
      </Badge>
    </li>
  );
}

/** Staff opening the workspace without permission, until marked as seen (and 30 days). */
function EmergencyList({
  workspaceId,
  emergency,
}: {
  workspaceId: string;
  emergency: EmergencyAccess[];
}) {
  const queryClient = useQueryClient();
  const unseen = emergency.some((item) => !item.acknowledged);
  const acknowledge = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/support-access/acknowledge", {
          params: { path: { workspace_id: workspaceId } },
        }),
      ),
    onSuccess: (data) => queryClient.setQueryData(supportAccessQuery(workspaceId).queryKey, data),
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <div className="flex flex-col gap-2 rounded-xl border border-low/40 bg-low/5 px-4 py-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-semibold">Emergency access without your permission</h3>
        {unseen && (
          <Button
            size="sm"
            variant="outline"
            onClick={() => acknowledge.mutate()}
            disabled={acknowledge.isPending}
          >
            Mark as seen
          </Button>
        )}
      </div>
      <ul className="flex flex-col gap-2 text-sm">
        {emergency.map((item) => (
          <li key={item.id} className="flex flex-col">
            <span className={item.acknowledged ? undefined : "font-semibold"}>“{item.reason}”</span>
            <span className="text-xs text-muted-foreground">
              {item.staff_name ?? "NOALBS staff"} ·{" "}
              {new Date(item.created_at).toLocaleString(currentLocale())},
              {SUPPORT_LEVEL_LABELS[item.level].toLowerCase()} until{" "}
              {new Date(item.expires_at).toLocaleTimeString(currentLocale())}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
