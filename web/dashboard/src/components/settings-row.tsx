import type { ReactNode } from "react";

export function SettingsRow({
  label,
  description,
  children,
}: {
  label: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-3 border-t border-divider px-5 py-3 sm:flex-row sm:items-center sm:justify-between sm:gap-6">
      <div className="min-w-0 flex-1">
        <div className="text-sm font-semibold">{label}</div>
        {description && (
          <div className="mt-1 text-xs text-muted-foreground wrap-anywhere">{description}</div>
        )}
      </div>
      {children && (
        <div className="flex min-w-0 flex-wrap items-center gap-2 sm:max-w-[65%]">{children}</div>
      )}
    </div>
  );
}
