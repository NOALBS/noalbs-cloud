import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { Plus } from "lucide-react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { adminNoticesQuery, queryKeys } from "@/api/queries";
import type { AdminNotice } from "@/api/types";
import { NoticeDialog } from "@/components/admin/notice-dialog";
import { ConfirmButton } from "@/components/billing/confirm-button";
import { Page, Section } from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { errorMessage } from "@/lib/errors";
import { currentLocale } from "@/lib/locale";
import { formatRelative } from "@/lib/time";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/_app/admin/notices")({
  component: NoticesPage,
});

const SEVERITY_CLASS: Record<string, string> = {
  info: "bg-primary-soft text-primary-soft-foreground",
  warning: "bg-low/15 text-low",
  critical: "bg-destructive/15 text-destructive-foreground",
};

const STATE_LABEL: Record<string, string> = {
  active: "Showing",
  upcoming: "Scheduled",
  ended: "Ended",
};

function NoticesPage() {
  const notices = useQuery(adminNoticesQuery);

  return (
    <Page
      title="Notices"
      description="Messages shown as a banner to everyone who is signed in: maintenance, an outage, a change. Each post, change and removal is in the audit log."
      className="max-w-4xl"
      actions={
        <NoticeDialog>
          <Button size="sm">
            <Plus className="size-4" />
            New notice
          </Button>
        </NoticeDialog>
      }
    >
      <Section title="All notices">
        {notices.isError && (
          <p className="px-5 pb-5 text-sm text-destructive">{errorMessage(notices.error)}</p>
        )}
        {notices.isPending && <Skeleton className="mx-5 mb-5 h-20" />}
        {notices.isSuccess && notices.data.length === 0 && (
          <p className="px-5 pb-5 text-sm text-muted-foreground">No notices yet.</p>
        )}
        {notices.data && notices.data.length > 0 && (
          <ul>
            {notices.data.map((notice) => (
              <NoticeRow key={notice.id} notice={notice} />
            ))}
          </ul>
        )}
      </Section>
    </Page>
  );
}

function NoticeRow({ notice }: { notice: AdminNotice }) {
  const queryClient = useQueryClient();
  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.adminNotices }),
      queryClient.invalidateQueries({ queryKey: queryKeys.notices }),
    ]);
  };
  const path = { params: { path: { notice_id: notice.id } } };
  const end = useMutation({
    mutationFn: () => unwrap(api.POST("/admin/notices/{notice_id}/end", path)),
    onSuccess: async () => {
      await refresh();
      toast.success(notice.state === "upcoming" ? "Notice removed" : "Notice ended");
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  const remove = useMutation({
    mutationFn: () => unwrap(api.DELETE("/admin/notices/{notice_id}", path)),
    onSuccess: async () => {
      await refresh();
      toast.success("Notice deleted");
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const when = (iso: string) => new Date(iso).toLocaleString(currentLocale());
  const schedule =
    notice.state === "upcoming"
      ? `Starts ${when(notice.starts_at)}`
      : notice.state === "ended" && notice.ends_at
        ? `Ended ${formatRelative(notice.ends_at)}`
        : notice.ends_at
          ? `Until ${when(notice.ends_at)}`
          : "Until ended";

  return (
    <li
      className={cn(
        "flex flex-wrap items-start gap-3 border-t border-divider px-5 py-3",
        notice.state === "ended" && "opacity-60",
      )}
    >
      <span className="flex min-w-0 flex-1 flex-col gap-1">
        <span className="flex flex-wrap items-center gap-2">
          <Badge className={SEVERITY_CLASS[notice.severity]}>{notice.severity}</Badge>
          <Badge variant="outline">{STATE_LABEL[notice.state] ?? notice.state}</Badge>
          <span className="text-xs text-muted-foreground">{schedule}</span>
        </span>
        <span className="text-sm break-words whitespace-pre-line">{notice.message}</span>
        <span className="text-xs text-muted-foreground">
          Posted {formatRelative(notice.created_at)}
          {notice.created_by_name && ` by ${notice.created_by_name}`}
          {!notice.dismissible && " · can’t be hidden"}
        </span>
      </span>
      <span className="flex items-center gap-1.5">
        <NoticeDialog notice={notice}>
          <Button variant="outline" size="sm">
            Edit
          </Button>
        </NoticeDialog>
        {notice.state !== "ended" && (
          <ConfirmButton
            variant="outline"
            title={notice.state === "upcoming" ? "Remove this notice?" : "End this notice?"}
            description="Users stop seeing it within a few minutes."
            confirmLabel={notice.state === "upcoming" ? "Remove" : "End"}
            disabled={end.isPending}
            onConfirm={() => end.mutate()}
          >
            {notice.state === "upcoming" ? "Remove" : "End now"}
          </ConfirmButton>
        )}
        {notice.state === "ended" && (
          <ConfirmButton
            variant="ghost"
            title="Delete this notice?"
            description="It disappears from this list. The audit log keeps that it existed."
            confirmLabel="Delete"
            disabled={remove.isPending}
            onConfirm={() => remove.mutate()}
          >
            Delete
          </ConfirmButton>
        )}
      </span>
    </li>
  );
}
