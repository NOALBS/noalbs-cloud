import { useQuery } from "@tanstack/react-query";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import type { MyLive, SwitcherSummary, Workspace } from "@/api/types";
import { useDocumentVisible } from "@/hooks/use-document-visible";
import type { Actor } from "@/lib/permissions";
import { compareItems, groupItems, normalizeMyLive, normalizeWorkspace } from "./model";

export interface VisibleSwitcherProps {
  workspace: Workspace;
  switchers?: SwitcherSummary[];
  actor: Actor;
}

export async function fetchVisibleSwitchers(signal: AbortSignal): Promise<MyLive> {
  const read = (page: number) =>
    unwrap(api.GET("/me/live", { params: { query: { page } }, signal }));
  const first = await read(1);
  const items = [...first.items];
  for (let page = 2; page <= Math.ceil(first.total / Math.max(1, first.per_page)); page++) {
    items.push(...(await read(page)).items);
  }
  return { ...first, items };
}

export function mergeVisibleSwitchers(
  personal: MyLive | undefined,
  { workspace, switchers, actor }: VisibleSwitcherProps,
) {
  const mine = personal ? normalizeMyLive(personal).items : [];
  const local = normalizeWorkspace(switchers ?? [], workspace, actor).items;
  const byId = new Map(mine.map((item) => [item.switcher_id, item]));
  const recoveredFromOfflineCap = workspace.role
    ? local.filter((item) => !byId.has(item.switcher_id)).length
    : 0;
  for (const item of local) byId.set(item.switcher_id, item);
  const items = [...byId.values()].sort(compareItems);
  const groups = groupItems(items);
  const unknownOffline = Math.max(
    0,
    (personal?.offline_total ?? 0) -
      mine.filter((item) => !item.connected).length -
      recoveredFromOfflineCap,
  );
  const offline = groups.offline.length + unknownOffline;
  return {
    items,
    groups,
    offline,
    total: items.length + unknownOffline,
    attention: groups.attention.length,
  };
}

export function useVisibleSwitchers(props: VisibleSwitcherProps, enabled = true) {
  const visible = useDocumentVisible();
  const personal = useQuery({
    queryKey: [...queryKeys.workspaces, "operator-visible-switchers"],
    queryFn: ({ signal }) => fetchVisibleSwitchers(signal),
    enabled: enabled && visible,
    refetchInterval: 10_000,
    refetchIntervalInBackground: false,
    staleTime: 10_000,
  });
  return mergeVisibleSwitchers(personal.data, props);
}
