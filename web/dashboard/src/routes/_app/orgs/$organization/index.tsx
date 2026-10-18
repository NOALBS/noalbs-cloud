import { createFileRoute, redirect } from "@tanstack/react-router";

export const Route = createFileRoute("/_app/orgs/$organization/")({
  beforeLoad: ({ params }) => {
    throw redirect({
      to: "/orgs/$organization/clients",
      params: { organization: params.organization },
    });
  },
});
