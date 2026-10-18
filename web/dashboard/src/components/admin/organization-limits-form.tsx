import type { ReactNode } from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { belowUsage, digitsOnly, type LimitsForm } from "@/lib/organizations";

const FIELDS: { key: keyof LimitsForm; label: string; hint: string }[] = [
  {
    key: "maxSwitchers",
    label: "Switchers in the pool",
    hint: "All client workspaces together. Can’t go below the switchers in use; a limit never stops a running one.",
  },
  { key: "maxStreamServers", label: "Stream servers per switcher", hint: "In every client." },
  { key: "maxMembers", label: "Customers per client", hint: "The team isn't counted." },
  { key: "historyDays", label: "Days of stream history", hint: "Kept for every client." },
];

/** What staff grant an organization: the pooled limits every client workspace shares. */
export function OrganizationLimitsForm({
  value,
  onChange,
  invalid,
  switchersInUse,
  children,
}: {
  value: LimitsForm;
  onChange: (value: LimitsForm) => void;
  /** Show a field in error when a number can't be read. */
  invalid: boolean;
  /** The switchers its clients have now: the pool can't be set below them. */
  switchersInUse?: number;
  children?: ReactNode;
}) {
  const pool = /^\d+$/.test(value.maxSwitchers.trim()) ? Number(value.maxSwitchers.trim()) : null;
  const poolTooLow = switchersInUse !== undefined && belowUsage(pool, switchersInUse);

  return (
    <div className="flex flex-col gap-4">
      <div className="grid gap-4 sm:grid-cols-2">
        {FIELDS.map((field) => (
          <div key={field.key} className="flex flex-col gap-1.5">
            <Label htmlFor={`limit-${field.key}`}>{field.label}</Label>
            <Input
              id={`limit-${field.key}`}
              value={value[field.key]}
              onChange={(event) =>
                onChange({ ...value, [field.key]: digitsOnly(event.target.value) })
              }
              inputMode="numeric"
              maxLength={6}
              aria-invalid={
                (invalid && !/^\d+$/.test(value[field.key].trim())) ||
                (field.key === "maxSwitchers" && poolTooLow)
              }
              className="h-10"
            />
            <span
              className={
                field.key === "maxSwitchers" && poolTooLow
                  ? "text-xs text-destructive"
                  : "text-xs text-muted-foreground"
              }
            >
              {field.key === "maxSwitchers" && poolTooLow
                ? `${switchersInUse} in use: delete some first, or keep at least that many.`
                : field.hint}
            </span>
          </div>
        ))}
      </div>
      {children}
    </div>
  );
}
