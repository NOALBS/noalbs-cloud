const MASK = "••••••••";

/**
 * A URL safe to show on stream: just its scheme. Keys and tokens live in the
 * path, query or `user:pass@`, and the host itself can be a home IP address.
 */
export function maskUrl(value: string): string {
  try {
    return `${new URL(value.trim()).protocol}//${MASK}`;
  } catch {
    return MASK;
  }
}
