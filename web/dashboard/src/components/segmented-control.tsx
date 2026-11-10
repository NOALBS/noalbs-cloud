import { useId } from "react";

export function SegmentedControl<T extends string>({
  label,
  value,
  options,
  onValueChange,
}: {
  label: string;
  value: T;
  options: readonly { value: T; label: string }[];
  onValueChange: (value: T) => void;
}) {
  const name = useId();
  return (
    <fieldset aria-label={label} className="flex max-w-full flex-wrap rounded-lg bg-muted p-0.5">
      {options.map((option) => (
        <label
          key={option.value}
          className="relative cursor-pointer rounded-md px-3 py-1.5 text-sm font-semibold text-muted-foreground has-checked:bg-card has-checked:text-foreground has-checked:shadow-sm has-focus-visible:outline-2 has-focus-visible:outline-ring"
        >
          <input
            className="sr-only"
            type="radio"
            name={name}
            value={option.value}
            checked={value === option.value}
            onChange={() => onValueChange(option.value)}
          />
          {option.label}
        </label>
      ))}
    </fieldset>
  );
}
