import type {
  Client,
  ClientCustomer,
  LiveSwitcher,
  Organization,
  OrganizationRole,
} from "@/api/types";

export const ORGANIZATION_ROLE_LABELS: Record<OrganizationRole, string> = {
  member: "Member",
  admin: "Admin",
};

export const ORGANIZATION_ROLE_DESCRIPTIONS: Record<OrganizationRole, string> = {
  member: "Runs the clients’ switchers, stream servers and settings.",
  admin: "Also creates and deletes clients, and manages the team.",
};

/** The most switchers one client may be allocated (as the API). */
export const MAX_ALLOCATION = 10_000;

/** What a new client starts with: one switcher each, so a customer is one slot of the pool. */
export const DEFAULT_ALLOCATION = 1;

/** How much of the switcher pool is used. */
export function poolUsage(organization: Pick<Organization, "limits" | "usage">) {
  const used = organization.usage.switchers;
  const max = organization.limits.max_switchers;
  return {
    used,
    max,
    remaining: Math.max(max - used, 0),
    /** Switchers over the pool (after the limit was lowered): they keep running, but no new ones fit. */
    over: Math.max(used - max, 0),
    full: used >= max,
  };
}

export type ClientState = "live" | "online" | "offline" | "suspended";

export const CLIENT_STATES: ClientState[] = ["live", "online", "offline", "suspended"];

/** The dot for a client: live, OBS connected, nothing connected, or suspended. */
export function clientState(client: Pick<Client, "streaming" | "obs_connected" | "suspended">) {
  if (client.suspended) return "suspended" satisfies ClientState;
  if (client.streaming) return "live" satisfies ClientState;
  if (client.obs_connected) return "online" satisfies ClientState;
  return "offline" satisfies ClientState;
}

export const CLIENT_STATE_LABELS: Record<ClientState, string> = {
  live: "Live",
  online: "OBS connected",
  offline: "Offline",
  suspended: "Suspended",
};

/** Whether the customer has joined yet (the organization runs the workspace either way). */
export function isClaimed(client: Pick<Client, "customers">): boolean {
  return client.customers > 0;
}

/**
 * Reads an allocation field: empty means no cap (`null`), a whole number from 0
 * to {@link MAX_ALLOCATION} is that cap, anything else is `undefined` (invalid).
 */
export function parseAllocation(text: string): number | null | undefined {
  const trimmed = text.trim();
  if (trimmed === "") return null;
  if (!/^\d+$/.test(trimmed)) return undefined;
  const value = Number(trimmed);
  return value <= MAX_ALLOCATION ? value : undefined;
}

/** What an allocation shows as in a list: the number, or that only the pool limits it. */
export function allocationLabel(allocation: number | null | undefined): string {
  return allocation == null ? "Pool" : String(allocation);
}

/** The link a customer opens to join their workspace. */
export function customerInviteUrl(origin: string, token: string): string {
  return `${origin}/invite/${token}`;
}

/** The link a new team member opens. */
export function teamInviteUrl(origin: string, token: string): string {
  return `${origin}/org-invite/${token}`;
}

/** The status dot's colour for each {@link ClientState}. */
export const CLIENT_STATE_DOT: Record<ClientState, string> = {
  live: "bg-live",
  online: "bg-primary",
  offline: "bg-idle",
  suspended: "bg-offline",
};

/** The limits form as typed: every field is text until {@link parseLimits} reads it. */
export interface LimitsForm {
  maxSwitchers: string;
  maxStreamServers: string;
  maxMembers: string;
  historyDays: string;
}

/** Ranges as the API checks them. */
const LIMIT_MAX = {
  maxSwitchers: 100_000,
  maxStreamServers: 1_000,
  maxMembers: 1_000,
  historyDays: 3_650,
} as const;

export const LIMIT_DEFAULTS: LimitsForm = {
  maxSwitchers: "10",
  maxStreamServers: "10",
  maxMembers: "3",
  historyDays: "90",
};

export function limitsForm(limits: Organization["limits"]): LimitsForm {
  return {
    maxSwitchers: String(limits.max_switchers),
    maxStreamServers: String(limits.max_stream_servers),
    maxMembers: String(limits.max_members),
    historyDays: String(limits.history_days),
  };
}

/** Reads the limits form for the API, or `null` while a field isn't a whole number in range. */
export function parseLimits(form: LimitsForm): Organization["limits"] | null {
  const read = (text: string, max: number): number | null => {
    if (!/^\d+$/.test(text.trim())) return null;
    const value = Number(text.trim());
    return value <= max ? value : null;
  };
  const maxSwitchers = read(form.maxSwitchers, LIMIT_MAX.maxSwitchers);
  const maxStreamServers = read(form.maxStreamServers, LIMIT_MAX.maxStreamServers);
  const maxMembers = read(form.maxMembers, LIMIT_MAX.maxMembers);
  const historyDays = read(form.historyDays, LIMIT_MAX.historyDays);
  if (
    maxSwitchers === null ||
    maxStreamServers === null ||
    maxMembers === null ||
    historyDays === null
  ) {
    return null;
  }
  return {
    max_switchers: maxSwitchers,
    max_stream_servers: maxStreamServers,
    max_members: maxMembers,
    history_days: historyDays,
  };
}

/**
 * Whether a switcher limit (the pool, or a client's allocation; `null` = no cap)
 * would sit below the switchers already in use. The API refuses that: a limit
 * never stops a running switcher, so capacity has to be freed first.
 */
export function belowUsage(limit: number | null | undefined, used: number): boolean {
  return limit != null && limit < used;
}

/** The clients page's address: what is searched, filtered and which page, read from the URL. */
export interface ClientsSearch {
  q?: string;
  page?: number;
  customer?: ClientCustomer;
  /** What the client is doing now. */
  state?: ClientState;
  /** Newest first when left out. */
  sort?: "name";
}

/** Reads `ClientsSearch` from an untrusted query string, dropping what doesn't fit. */
export function parseClientsSearch(search: Record<string, unknown>): ClientsSearch {
  const page = Number(search.page);
  return {
    q: typeof search.q === "string" && search.q ? search.q : undefined,
    page: Number.isInteger(page) && page > 1 ? page : undefined,
    customer:
      search.customer === "joined" || search.customer === "unclaimed" ? search.customer : undefined,
    state: CLIENT_STATES.find((state) => state === search.state),
    sort: search.sort === "name" ? "name" : undefined,
  };
}

/** An organization's limits other than the switcher pool, as the Limits card lists them. */
export function otherLimits(limits: Organization["limits"]) {
  return [
    {
      label: "Stream servers",
      value: String(limits.max_stream_servers),
      hint: "per switcher",
    },
    {
      label: "Customers",
      value: String(limits.max_members),
      hint: "per client",
    },
    {
      label: "History",
      value: `${limits.history_days} ${limits.history_days === 1 ? "day" : "days"}`,
      hint: "of stream history kept",
    },
  ];
}

/** What a number field keeps of what was typed or pasted: only the digits. */
export function digitsOnly(text: string): string {
  return text.replace(/\D/g, "");
}

/** The most clients a bulk action takes (as the API). */
export const BULK_MAX = 100;

/**
 * The names in a pasted list, one per line (blank lines dropped, whitespace
 * trimmed), and whether there are more than a bulk action takes.
 */
export function parseNames(text: string): { names: string[]; tooMany: boolean } {
  const names = text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  return { names: names.slice(0, BULK_MAX), tooMany: names.length > BULK_MAX };
}

/** How a bulk action went: how many items worked and how many didn't. */
export function bulkOutcome(items: { error?: unknown | null }[]) {
  const failed = items.filter((item) => item.error).length;
  return { ok: items.length - failed, failed };
}

/**
 * A CSV cell: quoted when it holds a comma, quote or line break. One that starts
 * with `=`, `+`, `-`, `@` or a tab would run as a formula when the file is opened
 * in a spreadsheet, and a client's name is whatever its customer renamed it to, so
 * it gets a `'` in front.
 */
export function csvCell(value: string): string {
  const safe = /^[=+\-@\t\r]/.test(value) ? `'${value}` : value;
  return /[",\r\n]/.test(safe) ? `"${safe.replaceAll('"', '""')}"` : safe;
}

/** The invite links of a bulk invite as CSV (client, URL, link, expires), for sending out. */
export function invitesCsv(
  items: { name: string; url_slug: string; token?: string | null; expires_at?: string | null }[],
  origin: string,
): string {
  const rows = items
    .filter((item) => item.token)
    .map((item) =>
      [item.name, item.url_slug, customerInviteUrl(origin, item.token ?? ""), item.expires_at ?? ""]
        .map(csvCell)
        .join(","),
    );
  return ["client,url,invite link,expires", ...rows].join("\n");
}

/** What the activity page can narrow to: a name and the kinds of event it lists. */
export const ACTIVITY_PRESETS = [
  { value: "all", label: "Everything", kinds: undefined },
  {
    value: "streams",
    label: "Streams",
    kinds: "stream_started,stream_stopped,offline_timeout,ending_timeout",
  },
  { value: "scenes", label: "Scene switches", kinds: "scene_switched" },
  {
    value: "problems",
    label: "Problems",
    kinds:
      "obs_disconnected,offline_timeout,ending_timeout,recording_scene_error,belabox_error,belabox_offline,moblin_disconnected,moblin_wrong_password",
  },
  {
    value: "controls",
    label: "Controls",
    kinds: "auto_switch_changed,feed_fixed,marker_created,source_toggled,input_muted",
  },
] as const;

export type ActivityPreset = (typeof ACTIVITY_PRESETS)[number]["value"];

/** The activity page's address: a client search, a preset and one client, from the URL. */
/** What the activity page shows: the switchers' events, or the changes people made. */
export type ActivityView = "changes";

export interface ActivitySearch {
  /** The switchers' events when left out. */
  view?: ActivityView;
  q?: string;
  show?: Exclude<ActivityPreset, "all">;
  client?: string;
}

export function parseActivitySearch(search: Record<string, unknown>): ActivitySearch {
  const preset = ACTIVITY_PRESETS.find(
    (candidate) => candidate.value !== "all" && candidate.value === search.show,
  );
  return {
    q: typeof search.q === "string" && search.q ? search.q : undefined,
    show: preset?.value === "all" ? undefined : preset?.value,
    client: typeof search.client === "string" && search.client ? search.client : undefined,
    view: search.view === "changes" ? "changes" : undefined,
  };
}

/** The live overview's address: a search, which switchers to list and the page. */
export interface LiveSearch {
  q?: string;
  page?: number;
  show?: "live" | "obs";
}

export function parseLiveSearch(search: Record<string, unknown>): LiveSearch {
  const page = Number(search.page);
  return {
    q: typeof search.q === "string" && search.q ? search.q : undefined,
    page: Number.isInteger(page) && page > 1 ? page : undefined,
    show: search.show === "live" || search.show === "obs" ? search.show : undefined,
  };
}

/** The tag and dot for a connected switcher on the live overview. */
export function liveTag(
  item: Pick<LiveSwitcher, "paused" | "obs_connected" | "streaming" | "state">,
): {
  tag: string;
  dot: string;
  className: string;
} {
  if (item.paused) return { tag: "PAUSED", dot: "bg-idle", className: "text-low" };
  if (!item.obs_connected) return { tag: "NO OBS", dot: "bg-low", className: "text-low" };
  if (!item.streaming) {
    return { tag: "IDLE", dot: "bg-primary", className: "text-muted-foreground" };
  }
  switch (item.state) {
    case "low":
      return { tag: "LOW", dot: "bg-low", className: "text-low" };
    case "offline":
      return { tag: "BRB", dot: "bg-offline", className: "text-offline" };
    default:
      return { tag: "LIVE", dot: "bg-live", className: "text-live" };
  }
}
