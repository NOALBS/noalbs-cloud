import type { ReactNode } from "react";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";

/** A labelled on/off setting, the whole row clickable. */
export function ToggleRow({
  id,
  title,
  description,
  checked,
  onCheckedChange,
  disabled,
  note,
}: {
  id: string;
  title: string;
  description: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  /** A short tag next to the title, e.g. "Needs chat bot". */
  note?: string;
}) {
  return (
    <label
      htmlFor={id}
      className={cn(
        "flex min-h-16 items-center gap-4 border-t border-divider px-5 py-3",
        disabled ? "cursor-default" : "cursor-pointer hover:bg-surface-2/50",
      )}
    >
      <span className="flex flex-1 flex-col gap-0.5">
        <span className="flex flex-wrap items-center gap-2 text-sm font-semibold">
          {title}
          {note && (
            <Badge variant="secondary" className="font-normal">
              {note}
            </Badge>
          )}
        </span>
        <span className="text-sm text-muted-foreground">{description}</span>
      </span>
      <Switch id={id} checked={checked} onCheckedChange={onCheckedChange} disabled={disabled} />
    </label>
  );
}

/** A bar pinned to the bottom of the viewport while a form has unsaved changes. */
export function SaveBar({
  dirty,
  saving,
  error,
  onSave,
  onDiscard,
  disabled,
}: {
  dirty: boolean;
  saving: boolean;
  error?: string | null;
  onSave: () => void;
  onDiscard: () => void;
  disabled?: boolean;
}) {
  if (!dirty) return null;

  return (
    <div className="sticky bottom-24 z-10 mx-auto flex w-full max-w-xl items-center gap-3 rounded-2xl border border-border-strong bg-popover py-2.5 pr-2.5 pl-4 shadow-2xl md:bottom-6">
      <span className={cn("flex-1 text-sm", error ? "text-destructive" : "text-muted-foreground")}>
        {error ?? "Unsaved changes"}
      </span>
      <button
        type="button"
        onClick={onDiscard}
        className="h-9 rounded-lg px-3 text-sm font-semibold hover:bg-surface-2"
      >
        Discard
      </button>
      <button
        type="button"
        onClick={onSave}
        disabled={saving || disabled}
        className="h-9 rounded-lg bg-primary px-4 text-sm font-semibold text-primary-foreground disabled:opacity-50"
      >
        {saving ? "Saving…" : "Save"}
      </button>
    </div>
  );
}

/** A read-only notice for people without permission to change a page. */
export function ReadOnlyNotice({ children }: { children?: ReactNode }) {
  return (
    <p className="rounded-xl border border-dashed px-4 py-3 text-sm text-muted-foreground">
      {children ?? "You can view these settings. Ask an admin of this workspace to change them."}
    </p>
  );
}
