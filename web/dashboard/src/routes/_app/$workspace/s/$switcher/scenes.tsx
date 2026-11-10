import { createFileRoute, redirect } from "@tanstack/react-router";
import { legacySwitcherPage } from "@/lib/legacy-switcher-page";
export const Route = createFileRoute("/_app/$workspace/s/$switcher/scenes")({
  beforeLoad: ({ params, location }) => {
    throw redirect({
      ...legacySwitcherPage("scenes", location.hash),
      params,
      search: (current) => current,
      replace: true,
    });
  },
});
