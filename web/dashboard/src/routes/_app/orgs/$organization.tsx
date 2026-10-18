import { createFileRoute, notFound, Outlet } from "@tanstack/react-router";
import { ApiRequestError } from "@/api/client";
import { organizationBySlugQuery } from "@/api/queries";
import { OrgShell } from "@/components/orgs/org-shell";
import { useOrganization } from "@/hooks/use-organization";

/** An organization's pages, addressed by its slug (`/orgs/acme-hosting/...`). */
export const Route = createFileRoute("/_app/orgs/$organization")({
  loader: async ({ context: { queryClient }, params }) => {
    try {
      await queryClient.ensureQueryData(organizationBySlugQuery(params.organization));
    } catch (error) {
      if (error instanceof ApiRequestError && error.status === 404) throw notFound();
      throw error;
    }
  },
  component: OrganizationLayout,
});

function OrganizationLayout() {
  const { organization, me } = useOrganization();

  return (
    <OrgShell organization={organization} me={me}>
      <Outlet />
    </OrgShell>
  );
}
