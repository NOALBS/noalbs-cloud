export const SLUG_MIN_LENGTH = 3;
export const SLUG_MAX_LENGTH = 50;

/**
 * Turns a workspace name into a URL slug the API accepts: lowercase letters,
 * digits and single dashes, e.g. "Nomad Live!" → "nomad-live".
 */
export function slugify(name: string): string {
  return name
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, SLUG_MAX_LENGTH)
    .replace(/-+$/g, "");
}

/** Takes an invite link or a bare token and returns the token. */
export function inviteTokenFrom(input: string): string | null {
  const trimmed = input.trim();
  if (!trimmed) return null;
  const match = trimmed.match(/invite\/([A-Za-z0-9_-]+)/);
  if (match?.[1]) return match[1];
  return /^[A-Za-z0-9_-]{20,}$/.test(trimmed) ? trimmed : null;
}
