import { Link } from "@tanstack/react-router";
import type { Workspace } from "@/api/types";
import { Section } from "@/components/page";

/**
 * Says who manages a client workspace, to its customers and to the
 * organization's own team. Nothing for an ordinary workspace.
 */
export function ManagedNotice({ workspace }: { workspace: Workspace }) {
  const organization = workspace.managed_by;
  if (!organization) return null;

  return (
    <Section title="Managed workspace">
      <div className="flex flex-col gap-2 px-5 pb-5 text-sm text-muted-foreground">
        {workspace.organization_role ? (
          <>
            <p>
              This is a client workspace of{" "}
              <span className="font-semibold text-foreground">{organization.name}</span>, which owns
              it and pays for it from its pool of switchers.
            </p>
            <Link
              to="/orgs/$organization/clients"
              params={{ organization: organization.url_slug }}
              className="w-fit font-medium text-primary hover:underline"
            >
              Open {organization.name}
            </Link>
          </>
        ) : (
          <>
            <p>
              <span className="font-semibold text-foreground">{organization.name}</span> manages
              this workspace. Their team can see and run everything in it, including your stream and
              its stats, and change its settings.
            </p>
            <p>
              You run it with them as an admin. You can leave any time, and you can still create
              workspaces of your own.
            </p>
          </>
        )}
      </div>
    </Section>
  );
}
