import { createFileRoute, redirect } from "@tanstack/react-router";
import { ApiRequestError } from "@/api/client";
import { meQuery } from "@/api/queries";
import { ADD_SERVER_PATH, captureAddServerFragment } from "@/lib/add-server";
import { safeRedirect } from "@/lib/redirect";

const LANDING_RETRY_KEY = "noalbs-landing-retry";

/**
 * Reloads `/` for the landing page, unless that was just tried (the stale
 * cookie couldn't be removed): then `false`, and the visitor goes to /login.
 */
function reloadForLanding(): boolean {
  try {
    const tried = Number(sessionStorage.getItem(LANDING_RETRY_KEY) ?? 0);
    if (Date.now() - tried < 30_000) return false;
    sessionStorage.setItem(LANDING_RETRY_KEY, String(Date.now()));
  } catch {
    return false;
  }
  window.location.replace("/");
  return true;
}

/** Everything behind sign-in. Sends signed-out visitors to /login. */
export const Route = createFileRoute("/_app")({
  beforeLoad: async ({ context, location }) => {
    // Before anything can redirect: sign-in passes the address through the API.
    if (location.pathname === ADD_SERVER_PATH) captureAddServerFragment();
    try {
      const me = await context.queryClient.ensureQueryData(meQuery);
      return { me };
    } catch (error) {
      if (error instanceof ApiRequestError && error.status === 401) {
        // At `/` with a sign-in cookie that no longer works: the API just removed it,
        // so load `/` again to get the landing page (once, so it can't loop).
        if (location.pathname === "/" && reloadForLanding()) {
          await new Promise(() => {});
        }
        // Come back here after signing in, unless it's just the start page.
        const back = safeRedirect(
          location.pathname === ADD_SERVER_PATH ? ADD_SERVER_PATH : location.href,
        );
        throw redirect({ to: "/login", search: back === "/" ? {} : { redirect: back } });
      }
      throw error;
    }
  },
});
