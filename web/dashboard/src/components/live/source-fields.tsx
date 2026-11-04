import { Trans } from "@lingui/react/macro";
import { ChevronDown } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";

// The small form pieces the source dialogs share.

export function Field({
  id,
  label,
  hint,
  children,
}: {
  id: string;
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      {children}
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  );
}

export function Toggle({
  id,
  label,
  checked,
  onChange,
}: {
  id: string;
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between gap-3">
      <Label htmlFor={id} className="font-normal">
        {label}
      </Label>
      <Switch id={id} checked={checked} onCheckedChange={onChange} />
    </div>
  );
}

/** A whole number field that lets its text be cleared while typing. */
export function NumberField({
  id,
  label,
  value,
  min,
  max,
  onChange,
}: {
  id: string;
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
}) {
  const [text, setText] = useState(String(value));
  return (
    <Field id={id} label={label}>
      <Input
        id={id}
        type="number"
        inputMode="numeric"
        min={min}
        max={max}
        step={1}
        value={text}
        onChange={(event) => {
          setText(event.target.value);
          const number = Number.parseInt(event.target.value, 10);
          if (Number.isFinite(number)) onChange(number);
        }}
        onBlur={() => setText(String(value))}
        required
      />
    </Field>
  );
}

export function MoreSettings({ children }: { children: ReactNode }) {
  return (
    <details className="group rounded-lg border px-3 py-2">
      <summary className="flex cursor-pointer list-none items-center justify-between text-sm font-medium">
        <Trans>More settings</Trans>
        <ChevronDown className="size-4 transition-transform group-open:rotate-180" aria-hidden />
      </summary>
      <div className="mt-3 flex flex-col gap-3">{children}</div>
    </details>
  );
}
