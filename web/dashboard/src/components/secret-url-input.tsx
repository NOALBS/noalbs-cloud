import { t } from "@lingui/core/macro";
import { Eye, EyeOff } from "lucide-react";
import { useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { maskUrl } from "@/lib/secret-url";

/**
 * A URL field that may hold a key. It shows only the scheme and host until
 * "Show" is pressed, so it doesn't leak when the page is on stream.
 */
export function SecretUrlInput({
  id,
  value,
  onChange,
  placeholder,
  label,
  mask = maskUrl,
  readOnly = false,
  startRevealed = false,
}: {
  id: string;
  /** What it is, lower case, for the show/hide buttons: "stats URL". */
  label: string;
  /** How to show the value while hidden; URLs keep their scheme. */
  mask?: (value: string) => string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  /** Can be shown and copied, not edited (NOALBS support with a view-only grant). */
  readOnly?: boolean;
  /** Start shown: a URL being typed in for the first time. */
  startRevealed?: boolean;
}) {
  const [revealed, setRevealed] = useState(startRevealed);
  const input = useRef<HTMLInputElement>(null);
  // Nothing to hide while it's empty (e.g. cleared to paste a new one).
  const hidden = !revealed && value !== "";

  const reveal = () => {
    setRevealed(true);
    requestAnimationFrame(() => input.current?.focus());
  };

  return (
    <div className="flex gap-2">
      {hidden ? (
        <button
          type="button"
          id={id}
          onClick={reveal}
          aria-label={t`Hidden ${label}. Show it`}
          className="flex h-9 min-w-0 flex-1 items-center rounded-md border bg-transparent px-3 text-left font-mono text-sm text-muted-foreground dark:bg-input/30"
        >
          <span className="truncate">{mask(value)}</span>
        </button>
      ) : (
        <Input
          ref={input}
          id={id}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={placeholder}
          readOnly={readOnly}
          className="min-w-0 flex-1 font-mono"
          spellCheck={false}
          autoComplete="off"
        />
      )}
      {value !== "" && (
        <Button
          type="button"
          variant="outline"
          size="icon"
          onClick={() => (hidden ? reveal() : setRevealed(false))}
          aria-label={hidden ? t`Show ${label}` : t`Hide ${label}`}
          aria-pressed={!hidden}
        >
          {hidden ? <Eye /> : <EyeOff />}
        </Button>
      )}
    </div>
  );
}
