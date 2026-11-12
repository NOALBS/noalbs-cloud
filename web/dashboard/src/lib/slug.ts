export const SLUG_MIN_LENGTH = 3;
export const SLUG_MAX_LENGTH = 50;

/** Lowercase letters and digits, with everything else between them as one dash. */
function dashed(text: string): string {
  return text
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-");
}

/**
 * Turns a workspace name into a URL slug the API accepts: lowercase letters,
 * digits and single dashes, e.g. "Nomad Live!" → "nomad-live".
 */
export function slugify(name: string): string {
  return dashed(name)
    .replace(/^-+|-+$/g, "")
    .slice(0, SLUG_MAX_LENGTH)
    .replace(/-+$/g, "");
}

/**
 * A slug as it is being typed: like {@link slugify}, but a dash at the end stays,
 * or the one in "nomad-live" could never be typed. The API refuses a slug that
 * still ends in one.
 */
export function slugInput(typed: string): string {
  return dashed(typed).replace(/^-+/, "").slice(0, SLUG_MAX_LENGTH);
}

/** Takes an invite link or a bare token and returns the token. */
export function inviteTokenFrom(input: string): string | null {
  const trimmed = input.trim();
  if (!trimmed) return null;
  const match = trimmed.match(/invite\/([A-Za-z0-9_-]+)/);
  if (match?.[1]) return match[1];
  return /^[A-Za-z0-9_-]{20,}$/.test(trimmed) ? trimmed : null;
}
