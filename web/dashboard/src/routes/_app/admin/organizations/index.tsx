import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { Plus } from "lucide-react";
import { type FormEvent, useState } from "react";
import { api, unwrap } from "@/api/client";
import { adminOrganizationsQuery, queryKeys } from "@/api/queries";
import { OrganizationLimitsForm } from "@/components/admin/organization-limits-form";
import { Page } from "@/components/page";
import { SlugField, useSlugStatus } from "@/components/slug-field";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { errorMessage } from "@/lib/errors";
import { LIMIT_DEFAULTS, type LimitsForm, parseLimits } from "@/lib/organizations";
import { formatDate } from "@/lib/time";

export const Route = createFileRoute("/_app/admin/organizations/")({
  component: OrganizationsPage,
});

function OrganizationsPage() {
  const organizations = useQuery(adminOrganizationsQuery);

  return (
    <Page
      title="Organizations"
      description="Companies that run client workspaces for their customers, paid from a pool you set."
      className="max-w-6xl"
      actions={
        <NewOrganization>
          <Button>
            <Plus />
            New organization
          </Button>
        </NewOrganization>
      }
    >
      {!organizations.data ? (
        <Skeleton className="h-48 rounded-2xl" />
      ) : organizations.data.length === 0 ? (
        <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
          No organizations yet.
        </p>
      ) : (
        <ul className="flex flex-col overflow-hidden rounded-2xl border bg-card">
          <li className="hidden grid-cols-[1fr_7rem_7rem_6rem_8rem] gap-4 border-b border-divider px-4 py-2 text-xs tracking-wider text-muted-foreground uppercase md:grid">
            <span>Organization</span>
            <span>Clients</span>
            <span>Switchers</span>
            <span>Team</span>
            <span>Created</span>
          </li>
          {organizations.data.map((organization) => (
            <li key={organization.id} className="border-b border-divider last:border-b-0">
              <Link
                to="/admin/organizations/$organization"
                params={{ organization: organization.id }}
                className="grid grid-cols-[1fr_auto] items-center gap-x-4 gap-y-1 px-4 py-3 hover:bg-surface-2 md:grid-cols-[1fr_7rem_7rem_6rem_8rem]"
              >
                <span className="flex min-w-0 flex-col">
                  <span className="flex items-center gap-2 truncate text-sm font-medium">
                    {organization.name}
                    {organization.suspension && <Badge variant="destructive">Suspended</Badge>}
                  </span>
                  <span className="truncate text-xs text-muted-foreground">
                    /{organization.url_slug}
                  </span>
                </span>
                <span className="hidden text-sm md:block">{organization.usage.clients}</span>
                <span className="hidden text-sm md:block">
                  {organization.usage.switchers} / {organization.limits.max_switchers}
                </span>
                <span className="hidden text-sm md:block">{organization.usage.team}</span>
                <span className="hidden text-sm text-muted-foreground md:block">
                  {formatDate(organization.created_at)}
                </span>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </Page>
  );
}

function NewOrganization({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [customSlug, setCustomSlug] = useState<string | null>(null);
  const [limits, setLimits] = useState<LimitsForm>(LIMIT_DEFAULTS);
  const slug = useSlugStatus(name, customSlug, false);
  const parsed = parseLimits(limits);
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const create = useMutation({
    mutationFn: () => {
      if (!parsed) throw new Error("Check the limits");
      return unwrap(
        api.POST("/admin/organizations", {
          body: { name: name.trim(), url_slug: slug.slug, limits: parsed },
        }),
      );
    },
    onSuccess: async (detail) => {
      await queryClient.invalidateQueries({ queryKey: queryKeys.adminOrganizations });
      setOpen(false);
      await navigate({
        to: "/admin/organizations/$organization",
        params: { organization: detail.organization.id },
      });
    },
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    create.mutate();
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent className="sm:max-w-xl">
        <form onSubmit={submit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>New organization</DialogTitle>
            <DialogDescription>
              Set what its clients may use in total. Invite its first admin from the next page.
            </DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-2">
            <Label htmlFor="organization-name">Name</Label>
            <Input
              id="organization-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              maxLength={60}
              autoFocus
              className="h-10"
            />
          </div>
          <SlugField
            status={slug}
            onChange={setCustomSlug}
            prefix={`${window.location.host}/orgs/`}
          />
          <OrganizationLimitsForm value={limits} onChange={setLimits} invalid={parsed === null} />
          {create.error && <p className="text-sm text-destructive">{errorMessage(create.error)}</p>}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button
              type="submit"
              disabled={!name.trim() || !slug.ready || !parsed || create.isPending}
            >
              {create.isPending ? "Creating…" : "Create"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
