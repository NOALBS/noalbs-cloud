import { useQuery } from "@tanstack/react-query";
import { AlertTriangle, Info, OctagonAlert, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { noticesQuery } from "@/api/queries";
import type { Notice } from "@/api/types";
import { currentLocale } from "@/lib/locale";
import { DISMISSED_KEY, parseDismissed, pruneDismissed, visibleNotices } from "@/lib/notices";
import { cn } from "@/lib/utils";

const STYLES = {
  info: { box: "border-primary/30 bg-primary-soft text-primary-soft-foreground", icon: "" },
  warning: { box: "border-low/40 bg-low/10", icon: "text-low" },
  critical: { box: "border-destructive/40 bg-destructive/10", icon: "text-destructive-foreground" },
};

function readDismissed(): string[] {
  try {
    return parseDismissed(localStorage.getItem(DISMISSED_KEY));
  } catch {
    return [];
  }
}

function writeDismissed(ids: string[]) {
  try {
    localStorage.setItem(DISMISSED_KEY, JSON.stringify(ids));
  } catch {
    // Private mode or blocked storage: it just comes back next time.
  }
}

/** The notices staff posted for everyone, above the app. Critical ones can't be hidden. */
export function NoticeBanners() {
  const notices = useQuery(noticesQuery);
  const [dismissed, setDismissed] = useState(readDismissed);

  // Forget hidden notices once they're over, so the stored list can't grow forever.
  useEffect(() => {
    if (!notices.data) return;
    setDismissed((current) => {
      const kept = pruneDismissed(current, notices.data);
      if (kept.length === current.length) return current;
      writeDismissed(kept);
      return kept;
    });
  }, [notices.data]);

  const dismiss = useCallback((id: string) => {
    setDismissed((current) => {
      const next = current.includes(id) ? current : [...current, id];
      writeDismissed(next);
      return next;
    });
  }, []);

  const shown = visibleNotices(notices.data ?? [], dismissed);
  if (shown.length === 0) return null;

  return (
    <section aria-label="Notices">
      {shown.map((notice) => (
        <NoticeRow key={notice.id} notice={notice} onDismiss={() => dismiss(notice.id)} />
      ))}
    </section>
  );
}

function NoticeRow({ notice, onDismiss }: { notice: Notice; onDismiss: () => void }) {
  const style =
    notice.severity in STYLES ? STYLES[notice.severity as keyof typeof STYLES] : STYLES.info;
  const Icon =
    notice.severity === "critical"
      ? OctagonAlert
      : notice.severity === "warning"
        ? AlertTriangle
        : Info;

  return (
    <div
      role={notice.severity === "info" ? "status" : "alert"}
      className={cn("flex items-start gap-x-3 border-b px-4 py-2.5 text-sm md:px-8", style.box)}
    >
      <Icon className={cn("mt-0.5 size-4 shrink-0", style.icon)} />
      <span className="min-w-0 flex-1 whitespace-pre-line break-words">
        {notice.message}
        {notice.ends_at && (
          <span className="ml-2 text-xs opacity-70">
            Until {new Date(notice.ends_at).toLocaleString(currentLocale())}
          </span>
        )}
      </span>
      {notice.dismissible && (
        <button
          type="button"
          onClick={onDismiss}
          aria-label="Dismiss"
          className="-my-1 -mr-2 flex size-8 shrink-0 items-center justify-center rounded-md opacity-70 hover:opacity-100"
        >
          <X className="size-4" />
        </button>
      )}
    </div>
  );
}
