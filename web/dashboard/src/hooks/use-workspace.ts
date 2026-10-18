import { useSuspenseQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { meQuery, workspaceBySlugQuery, workspacesQuery } from "@/api/queries";
import { actorOf } from "@/lib/permissions";

const workspaceRoute = getRouteApi("/_app/$workspace");

/**
 * The workspace from the URL (`/$workspace/...`), the signed-in user, and the
 * actor used for permission checks, plus your own workspaces for the menu. Only for use
 * under `/$workspace`, whose loader has fetched them (a staff member may be in a
 * workspace that isn't theirs).
 */
export function useWorkspace() {
  const { workspace: slug } = workspaceRoute.useParams();
  const { data: workspace } = useSuspenseQuery(workspaceBySlugQuery(slug));
  const { data: workspaces } = useSuspenseQuery(workspacesQuery);
  const { data: me } = useSuspenseQuery(meQuery);

  const actor = actorOf(workspace, me);

  return { workspace, workspaces, me, actor };
}
