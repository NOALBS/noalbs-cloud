import { Check, Copy, Eye, EyeOff } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { maskUrl } from "@/lib/secret-url";

/**
 * A read-only value with a copy button, e.g. a browser-source URL. A `secret`
 * one shows only its scheme and host until revealed; Copy always copies it all.
 */
export function CopyField({
  value,
  label,
  secret = false,
}: {
  value: string;
  label: string;
  secret?: boolean;
}) {
  const [copied, setCopied] = useState(false);
  const [revealed, setRevealed] = useState(false);
  const hidden = secret && !revealed;

  const copy = async () => {
    await navigator.clipboard.writeText(value);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="flex items-center gap-2 rounded-xl border bg-background py-1.5 pr-1.5 pl-3">
      <span className="sr-only">{label}:</span>
      <code className="min-w-0 flex-1 truncate font-mono text-xs">
        {hidden ? maskUrl(value) : value}
      </code>
      {secret && (
        <Button
          size="icon-sm"
          variant="ghost"
          onClick={() => setRevealed(!revealed)}
          aria-label={hidden ? `Show ${label}` : `Hide ${label}`}
          aria-pressed={!hidden}
        >
          {hidden ? <Eye /> : <EyeOff />}
        </Button>
      )}
      <Button size="sm" variant="outline" onClick={copy}>
        {copied ? <Check /> : <Copy />}
        {copied ? "Copied" : "Copy"}
      </Button>
    </div>
  );
}
