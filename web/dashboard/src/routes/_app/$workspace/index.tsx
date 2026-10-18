import { createFileRoute, redirect } from "@tanstack/react-router";
import { switchersQuery, workspaceBySlugQuery } from "@/api/queries";
import { lastSwitcherId } from "@/lib/last-switcher";
import { supportGated } from "@/lib/support";

/** A workspace opens on its last used switcher's Live screen, or the overview
 * while it has no switchers yet. */
export const Route = createFileRoute("/_app/$workspace/")({
  beforeLoad: async ({ context: { queryClient }, params }) => {
    const workspace = await queryClient
      .ensureQueryData(workspaceBySlugQuery(params.workspace))
      .catch(() => null);
    // The layout's loader shows "not found" for an unknown slug, and the
    // support gate for staff without access.
    if (!workspace || supportGated(workspace)) return;

    const switchers = await queryClient.ensureQueryData(switchersQuery(workspace.id));
    const remembered = lastSwitcherId(workspace.id);
    const switcher = switchers.find((item) => item.id === remembered) ?? switchers[0];
    if (switcher) {
      throw redirect({
        to: "/$workspace/s/$switcher/live",
        params: { workspace: params.workspace, switcher: switcher.id },
        replace: true,
      });
    }
    throw redirect({ to: "/$workspace/overview", params, replace: true });
  },
});
