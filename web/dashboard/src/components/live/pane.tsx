import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/** A pane's content: an open title row (no rule under it) over a body that scrolls on its own. */
export function Pane({
  title,
  hint,
  tools,
  label,
  className,
  children,
}: {
  title: ReactNode;
  /** Muted text beside the title. */
  hint?: ReactNode;
  /** Controls at the right of the title row. */
  tools?: ReactNode;
  label?: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section aria-label={label} className="flex min-h-0 flex-1 flex-col">
      <header className="mb-3.5 flex min-h-8 items-center gap-2">
        <h2 className="shrink-0 text-[15px] font-semibold tracking-tight">{title}</h2>
        {hint && <span className="min-w-0 truncate text-xs text-subtle-foreground">{hint}</span>}
        <span className="flex-1" />
        {tools}
      </header>
      <div className={cn("live-pane-body min-h-0 flex-1", className)}>{children}</div>
    </section>
  );
}

/** A small uppercase state beside a pane title, e.g. STREAMING. */
export function PaneState({ tone, children }: { tone?: string; children: ReactNode }) {
  return (
    <span className={cn("shrink-0 text-[10px] font-bold tracking-wider uppercase", tone)}>
      {children}
    </span>
  );
}

/** A number in its own tile with a label under it. */
export function StatTile({ value, label }: { value: ReactNode; label: ReactNode }) {
  return (
    <div className="min-w-0 rounded-xl border bg-muted px-3 py-2.5">
      <div className="truncate font-mono text-xl leading-tight font-medium tabular">{value}</div>
      <div className="mt-1 truncate text-[11px] text-subtle-foreground">{label}</div>
    </div>
  );
}
