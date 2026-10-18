import { useSuspenseQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { meQuery, organizationBySlugQuery } from "@/api/queries";

const organizationRoute = getRouteApi("/_app/orgs/$organization");

/**
 * The organization from the URL (`/orgs/$organization/...`) and the signed-in
 * user. Only for use under `/orgs/$organization`, whose loader has fetched it.
 * Admins create and delete clients and manage the team; members run the clients.
 */
export function useOrganization() {
  const { organization: slug } = organizationRoute.useParams();
  const { data: organization } = useSuspenseQuery(organizationBySlugQuery(slug));
  const { data: me } = useSuspenseQuery(meQuery);

  return { organization, me, isAdmin: organization.role === "admin" };
}
