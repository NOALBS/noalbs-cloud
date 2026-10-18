import { useQuery } from "@tanstack/react-query";
import { Check, Loader2, X } from "lucide-react";
import { useId } from "react";
import { slugAvailableQuery } from "@/api/queries";
import { Label } from "@/components/ui/label";
import { useDebouncedValue } from "@/hooks/use-debounced-value";
import { SLUG_MIN_LENGTH, slugify } from "@/lib/slug";
import { cn } from "@/lib/utils";

/** How a new workspace's URL stands: what it is and whether it can be used. */
export interface SlugStatus {
  slug: string;
  checking: boolean;
  /** Taken or not allowed; `message` says why. */
  unavailable: boolean;
  message: string | null;
  /** Long enough, checked and free: the form may submit. */
  ready: boolean;
  /** The API said it's free (never, for a URL that isn't checked). */
  verified: boolean;
}

/**
 * The URL of a new workspace: it follows `name` until `custom` is typed by hand,
 * and is checked for availability once you stop typing. Something with a URL
 * space of its own (an organization) passes `check: false`: the API refuses a
 * taken or reserved one when it's created.
 */
export function useSlugStatus(name: string, custom: string | null, check = true): SlugStatus {
  const slug = custom ?? slugify(name);
  const debounced = useDebouncedValue(slug);
  const availability = useQuery({
    ...slugAvailableQuery(debounced),
    enabled: check && debounced.length >= SLUG_MIN_LENGTH,
  });
  if (!check) {
    return {
      slug,
      checking: false,
      unavailable: false,
      message: null,
      ready: slug.length >= SLUG_MIN_LENGTH,
      verified: false,
    };
  }
  const checking = slug !== debounced || availability.isFetching;
  const unavailable = !checking && availability.data?.available === false;

  return {
    slug,
    checking,
    unavailable,
    message: availability.data?.message ?? null,
    ready:
      slug.length >= SLUG_MIN_LENGTH &&
      !checking &&
      !unavailable &&
      availability.data?.available === true,
    verified: !checking && availability.data?.available === true,
  };
}

/** The URL input with its availability line, for use with {@link useSlugStatus}. */
export function SlugField({
  status,
  onChange,
  prefix = `${window.location.host}/`,
}: {
  status: SlugStatus;
  onChange: (slug: string) => void;
  /** What comes before the slug in the address. */
  prefix?: string;
}) {
  const id = useId();

  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor={id}>URL</Label>
      <div className="flex h-10 items-center rounded-md border border-input bg-transparent pl-3 focus-within:ring-[3px] focus-within:ring-ring/50">
        <span className="font-mono text-sm text-muted-foreground">{prefix}</span>
        <input
          id={id}
          value={status.slug}
          onChange={(event) => onChange(slugify(event.target.value) || event.target.value)}
          className="h-full min-w-0 flex-1 bg-transparent pr-3 font-mono text-sm outline-none"
          spellCheck={false}
        />
      </div>
      <p
        className={cn(
          "flex min-h-5 items-center gap-1.5 text-xs",
          status.unavailable ? "text-destructive" : "text-muted-foreground",
        )}
      >
        {status.slug.length < SLUG_MIN_LENGTH ? (
          "At least 3 characters: letters, numbers and dashes."
        ) : status.checking ? (
          <>
            <Loader2 className="size-3.5 animate-spin" /> Checking…
          </>
        ) : status.unavailable ? (
          <>
            <X className="size-3.5" /> {status.message}
          </>
        ) : status.verified ? (
          <>
            <Check className="size-3.5 text-live" /> Available
          </>
        ) : null}
      </p>
    </div>
  );
}
