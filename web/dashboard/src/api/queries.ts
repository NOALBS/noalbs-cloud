import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { api, unwrap } from "./client";
import type {
  ActivityFilter,
  AdminUserFilter,
  ClientFilter,
  LiveFilter,
  MyWorkspacesFilter,
} from "./types";

/** A zoomed-in part of a stream, as ISO times. */
export interface Range {
  from: string;
  to: string;
}

/** Query keys, so mutations can invalidate exactly what they change. */
export const queryKeys = {
  me: ["me"] as const,
  providers: ["providers"] as const,
  sessions: ["sessions"] as const,
  plans: ["plans"] as const,
  addons: ["addons"] as const,
  admin: ["admin"] as const,
  adminOverview: ["admin", "overview"] as const,
  adminUsers: (filter: AdminUserFilter) => ["admin", "users", filter] as const,
  adminUser: (id: string) => ["admin", "user", id] as const,
  adminWorkspaces: (q: string, page: number, organization?: string) =>
    ["admin", "workspaces", q, page, organization] as const,
  adminWorkspace: (id: string) => ["admin", "workspace", id] as const,
  adminProducts: ["admin", "products"] as const,
  adminChatBots: ["admin", "chat-bots"] as const,
  adminStatus: ["admin", "status"] as const,
  adminAudit: (kind: string) => ["admin", "audit", kind] as const,
  adminNotices: ["admin", "notices"] as const,
  adminOrganizations: ["admin", "organizations"] as const,
  adminOrganization: (id: string) => ["admin", "organizations", id] as const,
  notices: ["notices"] as const,
  organizations: ["organizations"] as const,
  organization: (id: string) => ["organizations", id] as const,
  clients: (id: string) => ["organizations", id, "clients"] as const,
  organizationChatBots: (id: string) => ["organizations", id, "chat-bots"] as const,
  team: (id: string) => ["organizations", id, "team"] as const,
  teamInvites: (id: string) => ["organizations", id, "invites"] as const,
  teamInvite: (token: string) => ["organization-invite", token] as const,
  workspaces: ["workspaces"] as const,
  workspace: (id: string) => ["workspaces", id] as const,
  workspacePlan: (id: string) => ["workspaces", id, "plan"] as const,
  members: (id: string) => ["workspaces", id, "members"] as const,
  supportAccess: (id: string) => ["workspaces", id, "support-access"] as const,
  supportLog: (id: string) => ["workspaces", id, "support-access", "log"] as const,
  invites: (id: string) => ["workspaces", id, "invites"] as const,
  invite: (token: string) => ["invite", token] as const,
  slugAvailable: (slug: string) => ["slug-available", slug] as const,
  switchers: (workspaceId: string) => ["workspaces", workspaceId, "switchers"] as const,
  switcher: (workspaceId: string, switcherId: string) =>
    ["workspaces", workspaceId, "switchers", switcherId] as const,
  streamServers: (workspaceId: string, switcherId: string) =>
    ["workspaces", workspaceId, "switchers", switcherId, "stream-servers"] as const,
  chat: (workspaceId: string, switcherId: string) =>
    ["workspaces", workspaceId, "switchers", switcherId, "chat"] as const,
  chatBots: (workspaceId: string) => ["workspaces", workspaceId, "chat-bots"] as const,
  events: (workspaceId: string, switcherId: string) =>
    ["workspaces", workspaceId, "switchers", switcherId, "events"] as const,
  obsScenes: (workspaceId: string, switcherId: string) =>
    ["workspaces", workspaceId, "switchers", switcherId, "obs-scenes"] as const,
  obsSources: (workspaceId: string, switcherId: string) =>
    ["workspaces", workspaceId, "switchers", switcherId, "obs-sources"] as const,
  obsAudio: (workspaceId: string, switcherId: string) =>
    ["workspaces", workspaceId, "switchers", switcherId, "obs-audio"] as const,
  obsStats: (workspaceId: string, switcherId: string) =>
    ["workspaces", workspaceId, "switchers", switcherId, "obs-stats"] as const,
  belaboxDetails: (workspaceId: string, switcherId: string) =>
    ["workspaces", workspaceId, "switchers", switcherId, "belabox-details"] as const,
  streams: (workspaceId: string, switcherId: string) =>
    ["workspaces", workspaceId, "switchers", switcherId, "streams"] as const,
  stream: (workspaceId: string, switcherId: string, streamId: string, range?: Range) =>
    [...queryKeys.streams(workspaceId, switcherId), streamId, range ?? null] as const,
  streamRaw: (
    workspaceId: string,
    switcherId: string,
    streamId: string,
    serverId: string,
    range?: Range,
  ) =>
    [
      ...queryKeys.streams(workspaceId, switcherId),
      streamId,
      "raw",
      serverId,
      range ?? null,
    ] as const,
  streamMarkers: (workspaceId: string, switcherId: string, streamId: string) =>
    [...queryKeys.streams(workspaceId, switcherId), streamId, "markers"] as const,
  /** The Moblin Assistant URL and password (managers only). */
  moblinConnection: (workspaceId: string, switcherId: string) =>
    ["workspaces", workspaceId, "switchers", switcherId, "moblin-connection"] as const,
  /** A just-issued browser-source URL; only known right after it is created. */
  connectLink: (switcherId: string) => ["connect-link", switcherId] as const,
};

export const meQuery = queryOptions({
  queryKey: queryKeys.me,
  queryFn: () => unwrap(api.GET("/auth/me")),
});

export const providersQuery = queryOptions({
  queryKey: queryKeys.providers,
  queryFn: () => unwrap(api.GET("/auth/providers")),
  staleTime: Number.POSITIVE_INFINITY,
});

/** Where the signed-in user is signed in (browsers), most recently used first. */
export const sessionsQuery = queryOptions({
  queryKey: queryKeys.sessions,
  queryFn: () => unwrap(api.GET("/me/sessions")),
});

export const plansQuery = queryOptions({
  queryKey: queryKeys.plans,
  queryFn: () => unwrap(api.GET("/plans")),
  staleTime: 5 * 60_000,
});

export const addonsQuery = queryOptions({
  queryKey: queryKeys.addons,
  queryFn: () => unwrap(api.GET("/addons")),
  staleTime: 5 * 60_000,
});

/** The workspaces you're a member of (staff too: the rest are under Admin). */
export const workspacesQuery = queryOptions({
  queryKey: queryKeys.workspaces,
  queryFn: () => unwrap(api.GET("/workspaces")),
});

/**
 * A page of the workspaces you're a member of, searched and filtered on the server, with
 * what each is doing; refreshed every 15 s. Under `queryKeys.workspaces`, so creating or
 * leaving one refreshes it.
 */
export const myWorkspacesQuery = (filter: MyWorkspacesFilter = {}) =>
  queryOptions({
    queryKey: [...queryKeys.workspaces, "page", filter] as const,
    queryFn: () =>
      unwrap(
        api.GET("/me/workspaces", { params: { query: { ...filter, q: filter.q || undefined } } }),
      ),
    placeholderData: (previous) => previous,
    refetchInterval: 15_000,
  });

/** Your switchers that are connected now, from every workspace you're in; refreshed every 5 s. */
export const myLiveQuery = (filter: { q?: string; page?: number; filter?: LiveFilter }) =>
  queryOptions({
    queryKey: [...queryKeys.workspaces, "live", filter] as const,
    queryFn: () =>
      unwrap(api.GET("/me/live", { params: { query: { ...filter, q: filter.q || undefined } } })),
    placeholderData: (previous) => previous,
    refetchInterval: 5_000,
  });

/**
 * The workspace behind a `/$workspace` URL: yours, or any one for staff. Under
 * `queryKeys.workspaces`, so invalidating the list refreshes it too.
 */
export const workspaceBySlugQuery = (slug: string) =>
  queryOptions({
    queryKey: [...queryKeys.workspaces, "by-slug", slug] as const,
    queryFn: () =>
      unwrap(api.GET("/workspaces/by-slug/{url_slug}", { params: { path: { url_slug: slug } } })),
    retry: false,
  });

export const workspacePlanQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: queryKeys.workspacePlan(workspaceId),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/plan", {
          params: { path: { workspace_id: workspaceId } },
        }),
      ),
  });

export const membersQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: queryKeys.members(workspaceId),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/members", {
          params: { path: { workspace_id: workspaceId } },
        }),
      ),
  });

export const invitesQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: queryKeys.invites(workspaceId),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/invites", {
          params: { path: { workspace_id: workspaceId } },
        }),
      ),
  });

export const invitePreviewQuery = (token: string) =>
  queryOptions({
    queryKey: queryKeys.invite(token),
    queryFn: () => unwrap(api.GET("/invites/{token}", { params: { path: { token } } })),
  });

export const slugAvailableQuery = (slug: string) =>
  queryOptions({
    queryKey: queryKeys.slugAvailable(slug),
    queryFn: () => unwrap(api.GET("/workspaces/slug-available", { params: { query: { slug } } })),
    staleTime: 10_000,
  });

export const switchersQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: queryKeys.switchers(workspaceId),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers", {
          params: { path: { workspace_id: workspaceId } },
        }),
      ),
    // Carries each switcher's live status for the status dots.
    refetchInterval: 15_000,
  });

export const switcherQuery = (workspaceId: string, switcherId: string) =>
  queryOptions({
    queryKey: queryKeys.switcher(workspaceId, switcherId),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}", {
          params: { path: { workspace_id: workspaceId, switcher_id: switcherId } },
        }),
      ),
  });

export const streamServersQuery = (workspaceId: string, switcherId: string) =>
  queryOptions({
    queryKey: queryKeys.streamServers(workspaceId, switcherId),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers", {
          params: { path: { workspace_id: workspaceId, switcher_id: switcherId } },
        }),
      ),
  });

export const chatQuery = (workspaceId: string, switcherId: string) =>
  queryOptions({
    queryKey: queryKeys.chat(workspaceId, switcherId),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/chat", {
          params: { path: { workspace_id: workspaceId, switcher_id: switcherId } },
        }),
      ),
  });

/** Who speaks in the workspace's channels: its own bots and the shared ones. */
export const chatBotsQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: queryKeys.chatBots(workspaceId),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/chat-bot", {
          params: { path: { workspace_id: workspaceId } },
        }),
      ),
  });

const EVENTS_PAGE = 50;

/** The activity backlog, newest first, a page at a time (older pages via `before`). */
export const eventsQuery = (workspaceId: string, switcherId: string) =>
  infiniteQueryOptions({
    queryKey: queryKeys.events(workspaceId, switcherId),
    queryFn: ({ pageParam }) =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/events", {
          params: {
            path: { workspace_id: workspaceId, switcher_id: switcherId },
            query: { limit: EVENTS_PAGE, before: pageParam ?? undefined },
          },
        }),
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (page) =>
      page.length < EVENTS_PAGE ? null : (page[page.length - 1]?.created_at ?? null),
  });

/** The scenes in the switcher's OBS; fails with `obs_offline` while OBS isn't connected. */
export const obsScenesQuery = (workspaceId: string, switcherId: string) =>
  queryOptions({
    queryKey: queryKeys.obsScenes(workspaceId, switcherId),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/scenes", {
          params: { path: { workspace_id: workspaceId, switcher_id: switcherId } },
        }),
      ),
    retry: false,
    staleTime: 10_000,
  });

/**
 * What's on the scene on air. `scene` (the on-air scene's UUID or name) is part of the key,
 * so switching scenes fetches the new one's sources.
 */
export const obsSourcesQuery = (workspaceId: string, switcherId: string, scene: string | null) =>
  queryOptions({
    queryKey: [...queryKeys.obsSources(workspaceId, switcherId), scene] as const,
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/sources", {
          params: { path: { workspace_id: workspaceId, switcher_id: switcherId } },
        }),
      ),
    retry: false,
    staleTime: 5_000,
  });

/**
 * OBS's Audio Mixer for the scene on air. `scene` (the on-air scene's UUID or name) is part of
 * the key, so switching scenes fetches the new one's inputs.
 */
export const obsAudioQuery = (workspaceId: string, switcherId: string, scene: string | null) =>
  queryOptions({
    queryKey: [...queryKeys.obsAudio(workspaceId, switcherId), scene] as const,
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/audio", {
          params: { path: { workspace_id: workspaceId, switcher_id: switcherId } },
        }),
      ),
    retry: false,
    staleTime: 5_000,
  });
/** OBS's stats (frame rate, dropped frames, CPU, memory, disk); the caller polls. */
export const obsStatsQuery = (workspaceId: string, switcherId: string) =>
  queryOptions({
    queryKey: queryKeys.obsStats(workspaceId, switcherId),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/stats", {
          params: { path: { workspace_id: workspaceId, switcher_id: switcherId } },
        }),
      ),
    retry: false,
  });

/** The BELABOX settings dialog's data (WiFi, modems, stream settings); the dialog polls it while open. */
export const belaboxDetailsQuery = (workspaceId: string, switcherId: string) =>
  queryOptions({
    queryKey: queryKeys.belaboxDetails(workspaceId, switcherId),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/belabox/details", {
          params: { path: { workspace_id: workspaceId, switcher_id: switcherId } },
        }),
      ),
    retry: false,
  });

const STREAMS_PAGE = 20;

/** Past and running streams, newest first, a page at a time (older via `before`). */
export const streamsQuery = (workspaceId: string, switcherId: string) =>
  infiniteQueryOptions({
    queryKey: queryKeys.streams(workspaceId, switcherId),
    queryFn: ({ pageParam }) =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/streams", {
          params: {
            path: { workspace_id: workspaceId, switcher_id: switcherId },
            query: { limit: STREAMS_PAGE, before: pageParam ?? undefined },
          },
        }),
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (page) =>
      page.length < STREAMS_PAGE ? null : (page[page.length - 1]?.started_at ?? null),
  });

/** One stream's stats and activity, whole or zoomed into `range`. */
export const streamQuery = (
  workspaceId: string,
  switcherId: string,
  streamId: string,
  range?: Range,
) =>
  queryOptions({
    queryKey: queryKeys.stream(workspaceId, switcherId, streamId, range),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/streams/{stream_id}", {
          params: {
            path: { workspace_id: workspaceId, switcher_id: switcherId, stream_id: streamId },
            query: range ?? {},
          },
        }),
      ),
    // A running stream keeps growing.
    refetchInterval: (query) => (query.state.data?.stream.ended_at == null ? 30_000 : false),
  });

/** One server's raw stats during a stream (managers only). */
export const streamRawQuery = (
  workspaceId: string,
  switcherId: string,
  streamId: string,
  serverId: string,
  range?: Range,
) =>
  queryOptions({
    queryKey: queryKeys.streamRaw(workspaceId, switcherId, streamId, serverId, range),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/streams/{stream_id}/raw", {
          params: {
            path: { workspace_id: workspaceId, switcher_id: switcherId, stream_id: streamId },
            query: { server_id: serverId, ...range },
          },
        }),
      ),
  });

/** A stream's markers, in order. */
export const streamMarkersQuery = (workspaceId: string, switcherId: string, streamId: string) =>
  queryOptions({
    queryKey: queryKeys.streamMarkers(workspaceId, switcherId, streamId),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/streams/{stream_id}/markers", {
          params: {
            path: { workspace_id: workspaceId, switcher_id: switcherId, stream_id: streamId },
          },
        }),
      ),
  });

export const adminOverviewQuery = queryOptions({
  queryKey: queryKeys.adminOverview,
  queryFn: () => unwrap(api.GET("/admin/overview")),
});

/** Users (staff only), searched by name or ID, filtered by how recently they were active. */
export const adminUsersQuery = (filter: AdminUserFilter) =>
  queryOptions({
    queryKey: queryKeys.adminUsers(filter),
    queryFn: () =>
      unwrap(
        api.GET("/admin/users", {
          params: { query: { ...filter, q: filter.q || undefined } },
        }),
      ),
    placeholderData: (previous) => previous,
  });
export const adminUserQuery = (id: string) =>
  queryOptions({
    queryKey: queryKeys.adminUser(id),
    queryFn: () => unwrap(api.GET("/admin/users/{user_id}", { params: { path: { user_id: id } } })),
  });

/** Workspaces (staff only), newest first, searched by name, slug or ID. */
export const adminWorkspacesQuery = (q: string, page: number, organization?: string) =>
  queryOptions({
    queryKey: queryKeys.adminWorkspaces(q, page, organization),
    queryFn: () =>
      unwrap(
        api.GET("/admin/workspaces", {
          params: { query: { q: q || undefined, page, organization } },
        }),
      ),
    placeholderData: (previous) => previous,
  });

export const adminWorkspaceQuery = (id: string) =>
  queryOptions({
    queryKey: queryKeys.adminWorkspace(id),
    queryFn: () =>
      unwrap(
        api.GET("/admin/workspaces/{workspace_id}", {
          params: { path: { workspace_id: id } },
        }),
      ),
  });

/** What's running now (staff only); refreshed every 15 s while shown. */
export const adminStatusQuery = queryOptions({
  queryKey: queryKeys.adminStatus,
  queryFn: () => unwrap(api.GET("/admin/status")),
  refetchInterval: 15_000,
});

/** A page of an organization's client workspaces, for staff (no live state). */
export const adminOrganizationClientsQuery = (organizationId: string, filter: ClientFilter = {}) =>
  queryOptions({
    queryKey: [...queryKeys.adminOrganization(organizationId), "clients", filter] as const,
    queryFn: () =>
      unwrap(
        api.GET("/admin/organizations/{organization_id}/clients", {
          params: {
            path: { organization_id: organizationId },
            query: { ...filter, q: filter.q || undefined },
          },
        }),
      ),
    placeholderData: (previous) => previous,
  });

export const adminChatBotsQuery = queryOptions({
  queryKey: queryKeys.adminChatBots,
  queryFn: () => unwrap(api.GET("/admin/chat-bots")),
});

export const adminProductsQuery = queryOptions({
  queryKey: queryKeys.adminProducts,
  queryFn: () => unwrap(api.GET("/admin/products")),
});

/** Whether NOALBS support may come in now (owners, admins and staff). */
export const supportAccessQuery = (workspaceId: string) =>
  queryOptions({
    queryKey: queryKeys.supportAccess(workspaceId),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/support-access", {
          params: { path: { workspace_id: workspaceId } },
        }),
      ),
  });

const SUPPORT_LOG_PAGE = 20;

/** What NOALBS staff did in the workspace, newest first, paged back on `before`. */
export const supportLogQuery = (workspaceId: string) =>
  infiniteQueryOptions({
    queryKey: queryKeys.supportLog(workspaceId),
    queryFn: ({ pageParam }) =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/support-access/log", {
          params: {
            path: { workspace_id: workspaceId },
            query: { limit: SUPPORT_LOG_PAGE, before: pageParam ?? undefined },
          },
        }),
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (page) =>
      page.length < SUPPORT_LOG_PAGE ? null : (page[page.length - 1]?.created_at ?? null),
  });

const AUDIT_PAGE = 50;

/** What staff did (staff only), newest first; `kind` is `admin`, `support` or all. */
export const adminAuditQuery = (kind: "all" | "admin" | "support") =>
  infiniteQueryOptions({
    queryKey: queryKeys.adminAudit(kind),
    queryFn: ({ pageParam }) =>
      unwrap(
        api.GET("/admin/audit", {
          params: {
            query: {
              kind: kind === "all" ? undefined : kind,
              limit: AUDIT_PAGE,
              before: pageParam ?? undefined,
            },
          },
        }),
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (page) =>
      page.length < AUDIT_PAGE ? null : (page[page.length - 1]?.created_at ?? null),
  });

/** How often the banner re-reads the notices in force. */
const NOTICES_REFRESH_MS = 5 * 60 * 1000;

/** The notices in force, for the banner. */
export const noticesQuery = queryOptions({
  queryKey: queryKeys.notices,
  queryFn: () => unwrap(api.GET("/notices")),
  refetchInterval: NOTICES_REFRESH_MS,
  staleTime: NOTICES_REFRESH_MS / 5,
});

/** Every notice (staff only), newest first. */
export const adminNoticesQuery = queryOptions({
  queryKey: queryKeys.adminNotices,
  queryFn: () => unwrap(api.GET("/admin/notices")),
});

/** The organizations you're on the team of. */
export const organizationsQuery = queryOptions({
  queryKey: queryKeys.organizations,
  queryFn: () => unwrap(api.GET("/organizations")),
});

/** The organization behind an `/orgs/$organization` URL (its team only). */
export const organizationBySlugQuery = (slug: string) =>
  queryOptions({
    queryKey: [...queryKeys.organizations, "by-slug", slug] as const,
    queryFn: () =>
      unwrap(
        api.GET("/organizations/by-slug/{url_slug}", { params: { path: { url_slug: slug } } }),
      ),
    retry: false,
  });

/**
 * A page of an organization's client workspaces with their live state, searched
 * and filtered on the server; refreshed every 15 s. Invalidate by
 * `queryKeys.clients(id)`, which is every page's prefix.
 */
export const clientsQuery = (organizationId: string, filter: ClientFilter = {}) =>
  queryOptions({
    queryKey: [...queryKeys.clients(organizationId), filter] as const,
    queryFn: () =>
      unwrap(
        api.GET("/organizations/{organization_id}/clients", {
          params: {
            path: { organization_id: organizationId },
            query: { ...filter, q: filter.q || undefined },
          },
        }),
      ),
    placeholderData: (previous) => previous,
    refetchInterval: 15_000,
  });

/** The bots an organization lets answer for all its clients. */
export const organizationChatBotsQuery = (organizationId: string) =>
  queryOptions({
    queryKey: queryKeys.organizationChatBots(organizationId),
    queryFn: () =>
      unwrap(
        api.GET("/organizations/{organization_id}/chat-bots", {
          params: { path: { organization_id: organizationId } },
        }),
      ),
  });

/** What the organization's switchers are doing, connected ones a page at a time; refreshed every 5 s. */
export const organizationLiveQuery = (
  organizationId: string,
  filter: { q?: string; page?: number; filter?: LiveFilter },
) =>
  queryOptions({
    queryKey: [...queryKeys.organization(organizationId), "live", filter] as const,
    queryFn: () =>
      unwrap(
        api.GET("/organizations/{organization_id}/live", {
          params: {
            path: { organization_id: organizationId },
            query: { ...filter, q: filter.q || undefined },
          },
        }),
      ),
    placeholderData: (previous) => previous,
    refetchInterval: 5_000,
  });

const ACTIVITY_PAGE = 50;

/** The activity across all of an organization's clients, newest first (older pages via `before`). */
export const organizationActivityQuery = (
  organizationId: string,
  filter: Pick<ActivityFilter, "q" | "kinds" | "client">,
) =>
  infiniteQueryOptions({
    queryKey: [...queryKeys.organization(organizationId), "activity", filter] as const,
    queryFn: ({ pageParam }) =>
      unwrap(
        api.GET("/organizations/{organization_id}/activity", {
          params: {
            path: { organization_id: organizationId },
            query: {
              ...filter,
              q: filter.q || undefined,
              limit: ACTIVITY_PAGE,
              before: pageParam ?? undefined,
            },
          },
        }),
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (page) =>
      page.length < ACTIVITY_PAGE ? null : (page[page.length - 1]?.event.created_at ?? null),
    refetchInterval: 30_000,
  });

const ORG_AUDIT_PAGE = 50;

/** What the team, the customers and NOALBS changed in an organization, newest first. */
export const organizationAuditQuery = (organizationId: string) =>
  infiniteQueryOptions({
    queryKey: [...queryKeys.organization(organizationId), "audit"] as const,
    queryFn: ({ pageParam }) =>
      unwrap(
        api.GET("/organizations/{organization_id}/audit", {
          params: {
            path: { organization_id: organizationId },
            query: { limit: ORG_AUDIT_PAGE, before: pageParam ?? undefined },
          },
        }),
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (page) =>
      page.length < ORG_AUDIT_PAGE ? null : (page[page.length - 1]?.created_at ?? null),
  });

export const teamQuery = (organizationId: string) =>
  queryOptions({
    queryKey: queryKeys.team(organizationId),
    queryFn: () =>
      unwrap(
        api.GET("/organizations/{organization_id}/members", {
          params: { path: { organization_id: organizationId } },
        }),
      ),
  });

export const teamInvitesQuery = (organizationId: string) =>
  queryOptions({
    queryKey: queryKeys.teamInvites(organizationId),
    queryFn: () =>
      unwrap(
        api.GET("/organizations/{organization_id}/invites", {
          params: { path: { organization_id: organizationId } },
        }),
      ),
  });

export const teamInvitePreviewQuery = (token: string) =>
  queryOptions({
    queryKey: queryKeys.teamInvite(token),
    queryFn: () =>
      unwrap(api.GET("/organization-invites/{token}", { params: { path: { token } } })),
  });

/** Every organization (staff only). */
export const adminOrganizationsQuery = queryOptions({
  queryKey: queryKeys.adminOrganizations,
  queryFn: () => unwrap(api.GET("/admin/organizations")),
});

export const adminOrganizationQuery = (id: string) =>
  queryOptions({
    queryKey: queryKeys.adminOrganization(id),
    queryFn: () =>
      unwrap(
        api.GET("/admin/organizations/{organization_id}", {
          params: { path: { organization_id: id } },
        }),
      ),
  });
