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
      <span className="flex justify-between text-sm">
        <span className="font-semibold">{label}</span>
        <span className="tabular font-mono text-muted-foreground">{used}</span>
      </span>
    );
  }
  const full = used >= max;
  return (
    <div className="flex flex-col gap-1.5">
      <span className="flex justify-between text-sm">
        <span className="font-semibold">{label}</span>
        <span className="tabular font-mono text-muted-foreground">
          {used} / {max}
        </span>
      </span>
      <Progress
        value={Math.min(100, (used / Math.max(max, 1)) * 100)}
        className={full ? "[&>*]:bg-low" : "[&>*]:bg-primary"}
        aria-label={`${label}: ${used} of ${max}`}
      />
    </div>
  );
}
