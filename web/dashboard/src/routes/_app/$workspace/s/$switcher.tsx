import { createFileRoute, notFound, Outlet } from "@tanstack/react-router";
import { useEffect } from "react";
import { ApiRequestError } from "@/api/client";
import { switcherQuery, workspaceBySlugQuery } from "@/api/queries";
import { useSwitcher } from "@/hooks/use-switcher";
import { rememberSwitcher } from "@/lib/last-switcher";
import { supportGated } from "@/lib/support";

/** One switcher's pages. Navigation between them lives in the app shell. */
export const Route = createFileRoute("/_app/$workspace/s/$switcher")({
  loader: async ({ context: { queryClient }, params }) => {
    const workspace = await queryClient
      .ensureQueryData(workspaceBySlugQuery(params.workspace))
      .catch(() => null);
    if (!workspace) throw notFound();
    // The layout shows the support gate instead.
    if (supportGated(workspace)) return;

    try {
      await queryClient.ensureQueryData(switcherQuery(workspace.id, params.switcher));
    } catch (error) {
      if (error instanceof ApiRequestError && [400, 403, 404].includes(error.status)) {
        throw notFound();
      }
      throw error;
    }
  },
  component: SwitcherLayout,
});

function SwitcherLayout() {
  const { workspace, switcher } = useSwitcher();

  useEffect(() => rememberSwitcher(workspace.id, switcher.id), [workspace.id, switcher.id]);

  return <Outlet />;
}
