import { createFileRoute, redirect } from "@tanstack/react-router";
import { legacySwitcherPage } from "@/lib/legacy-switcher-page";
export const Route = createFileRoute("/_app/$workspace/s/$switcher/triggers")({
  beforeLoad: ({ params, location }) => {
    throw redirect({
      ...legacySwitcherPage("triggers", location.hash),
      params,
      search: (current) => current,
      replace: true,
    });
  },
});
