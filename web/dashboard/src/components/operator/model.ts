import type { components } from "@/api/schema";
import type {
  LiveSwitcher,
  MyLive,
  OrganizationLive,
  OrganizationRole,
  SwitcherStatus,
  SwitcherSummary,
  Workspace,
  WorkspaceRole,
} from "@/api/types";
import type { LiveSearch } from "@/lib/organizations";
import { type Actor, can } from "@/lib/permissions";

export type Attention = components["schemas"]["Attention"];
export type OperatorItem = LiveSwitcher & {
  connected: boolean;
  role?: WorkspaceRole | null;
  organization_role?: OrganizationRole | null;
  last_seen_at?: string | null;
  can_control: boolean;
};
export type OperatorCounts = {
  streaming: number;
  attention: number;
  connected: number;
  recording: number;
  workspaces?: number;
  clients?: number;
};
export type OperatorData = {
  items: OperatorItem[];
  counts: OperatorCounts;
  total: number;
  perPage: number;
  offlineTotal: number;
};
export type Group = "attention" | "streaming" | "idle" | "offline";
export function canUseRowActions(item: OperatorItem): boolean {
  return item.connected && item.can_control && item.obs_connected && !item.paused;
}

export function classifyAttention(
  status: Pick<SwitcherStatus, "streaming" | "state" | "state_since" | "obs_lost">,
): { attention: Attention | null; attention_since: string | null } {
  if (status.obs_lost?.stream_started_at)
    return { attention: "obs_lost", attention_since: status.obs_lost.since };
  const attention =
    status.streaming && status.state === "offline"
      ? "no_feed"
      : status.streaming && status.state === "low"
        ? "low"
        : null;
  return { attention, attention_since: attention ? (status.state_since ?? null) : null };
}

function offlineItem(row: components["schemas"]["OfflineSwitcherDto"]): OperatorItem {
  return {
    ...row,
    connected: false,
    can_control: false,
    obs_connected: false,
    streaming: false,
    recording: false,
    state: "idle",
    stream_started_at: null,
    scene: null,
    attention: null,
    attention_since: null,
    server: null,
  };
}

function liveData(
  data: MyLive | OrganizationLive,
  control: (item: LiveSwitcher) => boolean,
): OperatorData {
  return {
    items: [
      ...data.items.map((item) => ({ ...item, connected: true, can_control: control(item) })),
      ...data.offline.map(offlineItem),
    ],
    counts: {
      streaming: data.counts.live,
      attention: data.counts.attention,
      connected: data.counts.connected,
      recording: data.counts.recording,
      ...("workspaces" in data.counts
        ? { workspaces: data.counts.workspaces }
        : { clients: data.counts.clients }),
    },
    total: data.total,
    perPage: data.per_page,
    offlineTotal: data.offline_total,
  };
}

export function normalizeMyLive(data: MyLive): OperatorData {
  return liveData(data, (item) =>
    can({ role: (item as MyLive["items"][number]).role, staff: false }, "switcher.control"),
  );
}

export function normalizeOrganizationLive(
  data: OrganizationLive,
  role: OrganizationRole | null,
): OperatorData {
  const normalized = liveData(data, () =>
    can({ role: null, organization: role, staff: false }, "switcher.control"),
  );
  return {
    ...normalized,
    items: normalized.items.map((item) => ({ ...item, organization_role: role })),
  };
}

export function normalizeWorkspace(
  switchers: SwitcherSummary[],
  workspace: Pick<Workspace, "id" | "name" | "url_slug">,
  actor: Actor,
): OperatorData {
  const items = switchers.map((switcher): OperatorItem => {
    const status = switcher.status;
    return {
      switcher_id: switcher.id,
      switcher_name: switcher.name,
      workspace_id: workspace.id,
      workspace_name: workspace.name,
      workspace_slug: workspace.url_slug,
      connected: Boolean(status),
      can_control: can(actor, "switcher.control"),
      last_seen_at: switcher.last_seen_at,
      paused: switcher.paused || Boolean(status?.paused),
      obs_connected: status?.obs_connected ?? false,
      streaming: status?.streaming ?? false,
      recording: status?.recording ?? false,
      state: status?.state ?? "idle",
      stream_started_at: status?.stream_started_at,
      scene: status?.current_scene?.name,
      state_since: status?.state_since,
      stops_at: status?.stops_at,
      obs_lost: status?.obs_lost,
      ...(status ? classifyAttention(status) : { attention: null, attention_since: null }),
      server: switcher.server,
    };
  });
  const connected = items.filter((item) => item.connected);
  return {
    items,
    counts: {
      streaming: connected.filter((item) => item.streaming).length,
      attention: connected.filter((item) => item.attention).length,
      connected: connected.length,
      recording: connected.filter((item) => item.recording).length,
    },
    total: connected.length,
    perPage: Math.max(1, connected.length),
    offlineTotal: items.length - connected.length,
  };
}

export function groupOf(item: OperatorItem): Group {
  return !item.connected
    ? "offline"
    : item.attention
      ? "attention"
      : item.streaming
        ? "streaming"
        : "idle";
}

const order: Group[] = ["attention", "streaming", "idle", "offline"];
const timestamp = (value: string | null | undefined, fallback: number) =>
  value && Number.isFinite(Date.parse(value)) ? Date.parse(value) : fallback;

export function compareItems(a: OperatorItem, b: OperatorItem): number {
  const group = groupOf(a);
  const rank = order.indexOf(group) - order.indexOf(groupOf(b));
  if (rank) return rank;
  const time =
    group === "attention"
      ? timestamp(a.attention_since, Infinity) - timestamp(b.attention_since, Infinity)
      : group === "streaming"
        ? timestamp(a.stream_started_at, Infinity) - timestamp(b.stream_started_at, Infinity)
        : group === "offline"
          ? timestamp(b.last_seen_at, -Infinity) - timestamp(a.last_seen_at, -Infinity)
          : 0;
  return (
    (Number.isNaN(time) ? 0 : time) ||
    a.workspace_name.localeCompare(b.workspace_name) ||
    a.switcher_name.localeCompare(b.switcher_name) ||
    a.switcher_id.localeCompare(b.switcher_id)
  );
}

export function groupItems(
  items: OperatorItem[],
  search?: LiveSearch,
): Record<Group, OperatorItem[]> {
  const groups: Record<Group, OperatorItem[]> = {
    attention: [],
    streaming: [],
    idle: [],
    offline: [],
  };
  const q = search?.q?.trim().toLocaleLowerCase();
  for (const item of [...items].sort(compareItems)) {
    if (
      q &&
      ![item.switcher_name, item.workspace_name, item.workspace_slug].some((text) =>
        text.toLocaleLowerCase().includes(q),
      )
    )
      continue;
    if (
      item.connected &&
      ((search?.show === "live" && !item.streaming) ||
        (search?.show === "obs" && !item.obs_connected) ||
        (search?.show === "attention" && !item.attention))
    )
      continue;
    groups[groupOf(item)].push(item);
  }
  return groups;
}
