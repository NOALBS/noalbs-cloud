import { createFileRoute, redirect } from "@tanstack/react-router";

export const Route = createFileRoute("/_app/$workspace/s/$switcher/")({
  beforeLoad: ({ params }) => {
    throw redirect({ to: "/$workspace/s/$switcher/live", params });
  },
});
