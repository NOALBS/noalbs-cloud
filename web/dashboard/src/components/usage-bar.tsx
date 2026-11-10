import { t } from "@lingui/core/macro";
import { Progress } from "@/components/ui/progress";

/**
 * How much of a plan limit is used, e.g. "Members 3 / 10". Without a `max` (one the
 * viewer isn't shown) it's only the count.
 */
export function UsageBar({
  label,
  used,
  max,
}: {
  label: string;
  used: number;
  max: number | null | undefined;
}) {
  if (max == null) {
    return (
      <span className="flex flex-col gap-1.5 text-xs text-muted-foreground">
        <span>{label}</span>
        <span className="tabular font-display text-sm font-bold text-foreground">{used}</span>
      </span>
    );
  }
  const full = used >= max;
  return (
    <div className="flex flex-col gap-1.5">
      <span className="flex flex-col gap-1.5 text-xs text-muted-foreground">
        <span>{label}</span>
        <span className="flex items-baseline gap-2">
          <span className="tabular font-display text-sm font-bold text-foreground">
            {t`${used} of ${max}`}
          </span>
          {full && <span className="text-xs font-semibold text-low">{t`Full`}</span>}
        </span>
      </span>
      <Progress
        value={Math.min(100, (used / Math.max(max, 1)) * 100)}
        className="h-1 bg-muted [&>*]:bg-foreground/55"
        aria-label={full ? t`${label}: ${used} of ${max}, full` : t`${label}: ${used} of ${max}`}
      />
    </div>
  );
}
