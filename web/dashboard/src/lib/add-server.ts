import type { StreamServerKind } from "@/api/types";
import { kindInfo, STREAM_SERVER_KINDS } from "@/lib/stream-server-kinds";

/**
 * "Add to NOALBS Cloud" links: a stream server provider links to
 * `/add-server#kind=…&url=…` and the streamer picks the switcher to add it to.
 * The data sits in the fragment, which browsers never send, so stats URLs (and
 * the keys in them) stay out of our logs.
 */
export const ADD_SERVER_PATH = "/add-server";

/** Fields of a link besides `kind`, `url` and `name`. */
export const LINK_DETAIL_FIELDS = [
  "publisher",
  "application",
  "key",
  "listener_id",
  "username",
  "password",
  "api_key",
] as const;

export type LinkDetailField = (typeof LINK_DETAIL_FIELDS)[number];

export interface AddServerRequest {
  kind: StreamServerKind;
  url: string;
  name: string | null;
  /** Only the settings `kind` uses. */
  details: Partial<Record<LinkDetailField, string>>;
}

// The API's limits (stream_servers.rs, server_details.rs, validate.rs).
const MAX_URL = 2048;
const MAX_NAME = 60;
const MAX_DETAIL = 256;

/** The detail fields a kind of stream server uses. */
export function detailFieldsOf(kind: StreamServerKind): LinkDetailField[] {
  const info = kindInfo(kind);
  const fields: LinkDetailField[] = info.fields.map(({ field }) => field);
  if (info.basicAuth) fields.push("username", "password");
  if (info.apiKey) fields.push("api_key");
  return fields;
}

/** Reads a link's fragment (with or without the `#`). `null` when it can't be used. */
export function parseAddServerFragment(hash: string): AddServerRequest | null {
  const params = new URLSearchParams(hash.replace(/^#/, ""));
  const kind = params.get("kind")?.trim().toLowerCase();
  const url = params.get("url")?.trim();
  if (!kind || !url || url.length > MAX_URL) return null;
  if (!STREAM_SERVER_KINDS.some((info) => info.kind === kind)) return null;

  const details: AddServerRequest["details"] = {};
  for (const field of detailFieldsOf(kind as StreamServerKind)) {
    const value = params.get(field)?.trim();
    if (value) details[field] = value.slice(0, MAX_DETAIL);
  }

  const name = params.get("name")?.trim().slice(0, MAX_NAME);
  return { kind: kind as StreamServerKind, url, name: name || null, details };
}

/** The fragment for a link (without the `#`). The inverse of `parseAddServerFragment`. */
export function buildAddServerFragment(request: AddServerRequest): string {
  const params = new URLSearchParams();
  params.set("kind", request.kind);
  params.set("url", request.url.trim());
  if (request.name?.trim()) params.set("name", request.name.trim());
  for (const field of detailFieldsOf(request.kind)) {
    const value = request.details[field]?.trim();
    if (value) params.set(field, value);
  }
  return params.toString();
}

/** The host to show before the link is trusted; `null` unless it's a plain http(s)/ws(s) URL. */
export function displayHost(url: string): string | null {
  try {
    const parsed = new URL(url);
    return ["http:", "https:", "ws:", "wss:"].includes(parsed.protocol) ? parsed.host : null;
  } catch {
    return null;
  }
}

const STASH_KEY = "noalbs-add-server";

/**
 * Keeps the link's data in this tab while signing in. The sign-in round trip
 * passes the page's address through the API, which must not see the fragment.
 */
export function stashAddServer(request: AddServerRequest) {
  try {
    sessionStorage.setItem(STASH_KEY, JSON.stringify(request));
  } catch {
    // Storage can be unavailable (private mode); the link then has to be opened again.
  }
}

export function loadAddServer(): AddServerRequest | null {
  try {
    const raw = sessionStorage.getItem(STASH_KEY);
    // Parsed again, so a stale or edited entry can't carry anything the link couldn't.
    return raw ? parseAddServerFragment(buildAddServerFragment(JSON.parse(raw))) : null;
  } catch {
    return null;
  }
}

export function clearAddServer() {
  try {
    sessionStorage.removeItem(STASH_KEY);
  } catch {
    // See `stashAddServer`.
  }
}

/**
 * On `/add-server`: moves the fragment into the stash and removes it from the
 * address bar, so nothing later (sign-in redirects, copied URLs) carries it.
 */
export function captureAddServerFragment() {
  const { hash, pathname, search } = window.location;
  if (!hash) return;
  const request = parseAddServerFragment(hash);
  if (request) stashAddServer(request);
  window.history.replaceState(window.history.state, "", pathname + search);
}
