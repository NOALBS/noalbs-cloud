import { useQueryClient } from "@tanstack/react-query";
import { createFileRoute, notFound, Outlet } from "@tanstack/react-router";
import { useEffect } from "react";
import { ApiRequestError, api } from "@/api/client";
import { meQuery, queryKeys, workspaceBySlugQuery, workspacesQuery } from "@/api/queries";
import type { Me } from "@/api/types";
import { AppShell } from "@/components/app-shell";
import { SupportGate } from "@/components/support/support-gate";
import { useWorkspace } from "@/hooks/use-workspace";
import { isSupport } from "@/lib/permissions";
import { rememberWorkspace } from "@/lib/recent-workspaces";

/** Every page of a workspace, addressed by its slug (`/nomad-live/...`). */
export const Route = createFileRoute("/_app/$workspace")({
  loader: async ({ context: { queryClient }, params }) => {
    try {
      await Promise.all([
        queryClient.ensureQueryData(workspaceBySlugQuery(params.workspace)),
        // The workspace menu lists your own.
        queryClient.ensureQueryData(workspacesQuery),
      ]);
    } catch (error) {
      if (error instanceof ApiRequestError && error.status === 404) throw notFound();
      throw error;
    }
  },
  component: WorkspaceLayout,
});

function WorkspaceLayout() {
  const { workspace, workspaces, me, actor } = useWorkspace();
  useRememberWorkspace(workspace.id, workspace.url_slug, workspace.role !== null, me);
  // The menus list the workspaces opened last, once there are many.
  useEffect(() => {
    if (workspace.role !== null) rememberWorkspace(workspace.id);
  }, [workspace.id, workspace.role]);
  useSupportExpiry(workspace.support_access?.expires_at ?? null);

  if (isSupport(actor) && !workspace.support_access) {
    return <SupportGate workspace={workspace} />;
  }

  return (
    <AppShell workspace={workspace} workspaces={workspaces} me={me}>
      <Outlet />
    </AppShell>
  );
}

/** Re-reads the workspace when support's permission runs out, so the gate shows. */
function useSupportExpiry(expiresAt: string | null) {
  const queryClient = useQueryClient();

  useEffect(() => {
    if (!expiresAt) return;
    const timer = window.setTimeout(
      () => void queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
      // setTimeout's limit is ~24.8 days; grants last at most 7.
      Math.max(Date.parse(expiresAt) - Date.now(), 0) + 1000,
    );
    return () => window.clearTimeout(timer);
  }, [expiresAt, queryClient]);
}

/** Saves this as the user's last workspace, so `/` brings them back here. Staff
 * looking at a workspace they aren't in are skipped. */
function useRememberWorkspace(workspaceId: string, slug: string, isMember: boolean, me: Me) {
  const queryClient = useQueryClient();

  useEffect(() => {
    if (!isMember || me.last_workspace_slug === slug) return;

    void api
      .PUT("/me/last-workspace", { body: { workspace_id: workspaceId } })
      .then(({ response }) => {
        if (response.ok) {
          queryClient.setQueryData(meQuery.queryKey, (current) =>
            current ? { ...current, last_workspace_slug: slug } : current,
          );
        }
      });
  }, [workspaceId, slug, isMember, me.last_workspace_slug, queryClient]);
}
