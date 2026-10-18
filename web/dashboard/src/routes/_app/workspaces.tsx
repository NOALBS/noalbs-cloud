import { useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, Outlet } from "@tanstack/react-router";
import { meQuery } from "@/api/queries";
import { HomeShell } from "@/components/home-shell";

/** The pages that span all your workspaces: the list (`/workspaces`) and the live overview. */
export const Route = createFileRoute("/_app/workspaces")({
  component: WorkspacesLayout,
});

function WorkspacesLayout() {
  const { data: me } = useSuspenseQuery(meQuery);

  return (
    <HomeShell me={me}>
      <Outlet />
    </HomeShell>
  );
}
