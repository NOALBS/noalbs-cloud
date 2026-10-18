import { createFileRoute, redirect } from "@tanstack/react-router";
import { api, unwrap } from "@/api/client";

/** `/` opens the last workspace, else the first one, else first-run. */
export const Route = createFileRoute("/_app/")({
  beforeLoad: async () => {
    // Read fresh from the API: cached data could send someone back into a
    // workspace they just left. Deliberately not written to the query cache,
    // so pages that are still unmounting don't re-render with it.
    const me = await unwrap(api.GET("/auth/me"));
    const slug = me.last_workspace_slug ?? (await unwrap(api.GET("/workspaces")))[0]?.url_slug;

    if (slug) {
      throw redirect({ to: "/$workspace", params: { workspace: slug } });
    }
    throw redirect({ to: "/welcome" });
  },
});
