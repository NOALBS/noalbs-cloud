import type { ReactNode } from "react";
import { Label } from "@/components/ui/label";
import { signalBars } from "@/lib/belabox";
import { cn } from "@/lib/utils";

/** A labelled control with a hint and/or an error under it. */
export function Field({
  label,
  htmlFor,
  hint,
  error,
  className,
  children,
}: {
  label: string;
  htmlFor?: string;
  hint?: ReactNode;
  error?: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={cn("flex min-w-0 flex-col gap-1.5", className)}>
      <Label htmlFor={htmlFor} className="text-xs tracking-wider text-muted-foreground uppercase">
        {label}
      </Label>
      {children}
      {error ? (
        <span className="text-xs text-destructive-foreground">{error}</span>
      ) : (
        hint && <span className="text-xs text-muted-foreground">{hint}</span>
      )}
    </div>
  );
}

/** Four bars for a 0–100 signal. */
export function SignalBars({
  signal,
  className,
}: {
  signal: number | null | undefined;
  className?: string;
}) {
  const bars = signalBars(signal);
  return (
    <span
      role="img"
      aria-label={signal != null ? `Signal ${signal}%` : "No signal"}
      className={cn("flex h-3.5 items-end gap-px", className)}
    >
      {[1, 2, 3, 4].map((bar) => (
        <span
          key={bar}
          style={{ height: `${bar * 25}%` }}
          className={cn("w-[3px] rounded-[1px]", bar <= bars ? "bg-live" : "bg-border-strong")}
        />
      ))}
    </span>
  );
}

/** A titled block of the dialog. */
export function Section({
  title,
  description,
  action,
  children,
}: {
  title: string;
  description?: ReactNode;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="flex flex-col gap-3 rounded-xl border border-divider p-3.5">
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-0.5">
          <h3 className="text-sm font-semibold">{title}</h3>
          {description && <p className="text-xs text-muted-foreground">{description}</p>}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}
