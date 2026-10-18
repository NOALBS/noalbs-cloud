import { cn } from "@/lib/utils";

const TONES = [
  "bg-primary-soft text-primary-soft-foreground",
  "bg-live/15 text-live",
  "bg-low/15 text-low",
  "bg-sky-500/15 text-sky-500",
];

export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  const letters = words.length > 1 ? `${words[0]?.[0]}${words[1]?.[0]}` : name.slice(0, 2);
  return letters.toUpperCase();
}

function toneFor(key: string): string {
  let hash = 0;
  for (const char of key) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return TONES[hash % TONES.length] ?? TONES[0] ?? "";
}

const SIZES = {
  sm: "size-6 rounded-md text-[10px]",
  md: "size-7 rounded-lg text-xs",
  lg: "size-12 rounded-xl text-base",
};

/** A workspace's initials on a colour picked from its slug, like the mockup. */
export function WorkspaceTile({
  seed,
  name,
  size = "md",
}: {
  /** Picks the colour; use the workspace slug so it matches everywhere. */
  seed: string;
  name: string;
  size?: keyof typeof SIZES;
}) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "flex shrink-0 items-center justify-center font-bold",
        SIZES[size],
        toneFor(seed),
      )}
    >
      {initials(name)}
    </span>
  );
}
