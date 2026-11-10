import type { ComponentProps, ReactNode } from "react";
import { cn } from "@/lib/utils";

export function RowList({ className, ...props }: ComponentProps<"ul">) {
  return <ul className={cn("flex flex-col", className)} {...props} />;
}

export function Row({
  lead,
  title,
  meta,
  children,
  className,
}: {
  lead?: ReactNode;
  title: ReactNode;
  meta?: ReactNode;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <li
      className={cn(
        "flex flex-wrap items-center gap-3 border-t border-divider px-5 py-3",
        className,
      )}
    >
      {lead && <div className="shrink-0">{lead}</div>}
      <div className="min-w-0 flex-1 basis-40">
        <div className="text-sm font-semibold wrap-anywhere">{title}</div>
        {meta && <div className="mt-0.5 text-xs text-muted-foreground wrap-anywhere">{meta}</div>}
      </div>
      {children && (
        <div className="flex w-full flex-wrap items-center gap-2 sm:w-auto">{children}</div>
      )}
    </li>
  );
}
