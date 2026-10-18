import { useSuspenseQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { switcherQuery } from "@/api/queries";
import { can, canReadSecrets } from "@/lib/permissions";
import { useWorkspace } from "./use-workspace";

const switcherRoute = getRouteApi("/_app/$workspace/s/$switcher");

/** The switcher from the URL, plus everything `useWorkspace()` gives. */
export function useSwitcher() {
  const workspaceContext = useWorkspace();
  const { switcher: switcherId } = switcherRoute.useParams();
  const { data: switcher } = useSuspenseQuery(
    switcherQuery(workspaceContext.workspace.id, switcherId),
  );

  return {
    ...workspaceContext,
    switcher,
    canManage: can(workspaceContext.actor, "switcher.manage"),
    /** Stats URLs, raw stats, the Moblin password: never shown to NOALBS support. */
    canReadSecrets: canReadSecrets(workspaceContext.actor),
    /** Path params for this switcher's API endpoints. */
    path: { workspace_id: workspaceContext.workspace.id, switcher_id: switcher.id },
  };
}
