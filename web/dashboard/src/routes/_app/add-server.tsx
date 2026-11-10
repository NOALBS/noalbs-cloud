import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQuery, useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { TriangleAlert } from "lucide-react";
import { type FormEvent, useState } from "react";
import { meQuery, switchersQuery, workspacesQuery } from "@/api/queries";
import { MinimalHeader } from "@/components/minimal-header";
import { NoticeBanners } from "@/components/notice-banner";
import { SecretUrlInput } from "@/components/secret-url-input";
import { detailsForm, detailsInput, ServerDetailsFields } from "@/components/server-details-fields";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { WorkspaceTile } from "@/components/workspace-tile";
import { useCreateStreamServer } from "@/hooks/use-create-stream-server";
import {
  type AddServerRequest,
  clearAddServer,
  displayHost,
  loadAddServer,
} from "@/lib/add-server";
import { errorMessage } from "@/lib/errors";
import { lastSwitcherId, rememberSwitcher } from "@/lib/last-switcher";
import { actorOf, can } from "@/lib/permissions";
import { kindInfo } from "@/lib/stream-server-kinds";

export const Route = createFileRoute("/_app/add-server")({
  loader: ({ context }) => context.queryClient.ensureQueryData(workspacesQuery),
  component: AddServerPage,
});

/**
 * Where "Add to NOALBS Cloud" links from stream server providers land. The
 * link's data was moved out of the address into this tab's storage by `_app`.
 */
function AddServerPage() {
  const { data: me } = useSuspenseQuery(meQuery);
  // Read once: the stash is cleared when the server is added or this is cancelled.
  const [request] = useState(loadAddServer);

  return (
    <div className="flex min-h-dvh flex-col">
      <MinimalHeader me={me} />
      <NoticeBanners />
      <main className="flex flex-1 items-center justify-center px-4 py-10">
        <section className="flex w-full max-w-130 flex-col gap-5.5 rounded-3xl border bg-card p-6 md:p-8">
          {request ? (
            <AddServerForm request={request} />
          ) : (
            <div className="flex flex-col gap-3">
              <h1 className="text-lg font-semibold">
                <Trans>Nothing to add</Trans>
              </h1>
              <p className="text-sm text-muted-foreground">
                <Trans>
                  This page opens from an “Add to NOALBS Cloud” link on a stream server’s website.
                  The link may be incomplete, or this tab was closed on the way. Open the link
                  again, or add the server by hand on a switcher’s Stream servers page.
                </Trans>
              </p>
              <Button asChild variant="outline" className="h-11 rounded-xl">
                <Link to="/">
                  <Trans>Go to my workspaces</Trans>
                </Link>
              </Button>
            </div>
          )}
        </section>
      </main>
    </div>
  );
}

function AddServerForm({ request }: { request: AddServerRequest }) {
  const { data: me } = useSuspenseQuery(meQuery);
  const { data: workspaces } = useSuspenseQuery(workspacesQuery);
  const navigate = useNavigate();

  const info = kindInfo(request.kind);
  const [name, setName] = useState(
    request.name ?? (request.kind === "belabox" ? "BELABOX Cloud" : info.label),
  );
  const [statsUrl, setStatsUrl] = useState(request.url);
  const [details, setDetails] = useState(() => ({ ...detailsForm(), ...request.details }));
  const [workspaceChoice, setWorkspaceChoice] = useState<string | null>(null);
  const [switcherChoice, setSwitcherChoice] = useState<string | null>(null);

  // Only workspaces where this person may manage switchers.
  const eligible = workspaces.filter((workspace) => can(actorOf(workspace, me), "switcher.manage"));
  const workspace =
    eligible.find((candidate) => candidate.id === workspaceChoice) ??
    eligible.find((candidate) => candidate.url_slug === me.last_workspace_slug) ??
    eligible[0];

  const switchers = useQuery({
    ...switchersQuery(workspace?.id ?? ""),
    enabled: Boolean(workspace),
  });
  const switcher =
    switchers.data?.find((candidate) => candidate.id === switcherChoice) ??
    switchers.data?.find((candidate) => candidate.id === lastSwitcherId(workspace?.id ?? "")) ??
    switchers.data?.[0];

  const create = useCreateStreamServer({
    workspace_id: workspace?.id ?? "",
    switcher_id: switcher?.id ?? "",
  });

  const host = displayHost(request.url);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!workspace || !switcher) return;
    create.mutate(
      { name, kind: request.kind, stats_url: statsUrl, details: detailsInput(details) },
      {
        onSuccess: async () => {
          clearAddServer();
          rememberSwitcher(workspace.id, switcher.id);
          await navigate({
            to: "/$workspace/s/$switcher/switching",
            hash: "servers",
            params: { workspace: workspace.url_slug, switcher: switcher.id },
          });
        },
      },
    );
  };

  return (
    <form onSubmit={submit} className="flex flex-col gap-5">
      <div className="flex flex-col gap-1">
        <span className="text-sm text-muted-foreground">
          <Trans>Add a stream server</Trans>
        </span>
        <h1 className="text-2xl font-bold">{info.label}</h1>
        <span className="text-sm text-muted-foreground">
          {host ? (
            <>
              <Trans>
                at <span className="font-mono text-foreground">{host}</span>
              </Trans>
            </>
          ) : (
            t`The address in this link doesn’t look right.`
          )}
        </span>
      </div>

      <p className="flex gap-2.5 rounded-2xl border border-divider bg-sunken p-4 text-sm text-muted-foreground">
        <TriangleAlert className="mt-0.5 size-4 shrink-0 text-low" />
        <span>
          <Trans>Only add servers you stream to</Trans>
        </span>
      </p>

      {eligible.length === 0 ? (
        <div className="flex flex-col gap-3">
          <p className="text-sm text-muted-foreground">
            <Trans>
              You aren’t an admin or owner of any workspace, so you can’t add stream servers. Create
              a workspace, or ask an admin to add it.
            </Trans>
          </p>
          <Button asChild variant="outline" className="h-11 rounded-xl">
            <Link to="/welcome" search={{ mode: "create" }}>
              <Trans>Create a workspace</Trans>
            </Link>
          </Button>
        </div>
      ) : (
        <>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="flex flex-col gap-2">
              <Label htmlFor="add-workspace">
                <Trans>Workspace</Trans>
              </Label>
              <Select
                value={workspace?.id}
                onValueChange={(next) => {
                  setWorkspaceChoice(next);
                  setSwitcherChoice(null);
                }}
              >
                <SelectTrigger id="add-workspace" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {eligible.map((candidate) => (
                    <SelectItem key={candidate.id} value={candidate.id}>
                      <span className="flex items-center gap-2">
                        <WorkspaceTile seed={candidate.url_slug} name={candidate.name} size="sm" />
                        {candidate.name}
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="flex flex-col gap-2">
              <Label htmlFor="add-switcher">
                <Trans>Switcher</Trans>
              </Label>
              {switchers.isPending ? (
                <Skeleton className="h-9" />
              ) : switchers.data?.length ? (
                <Select value={switcher?.id} onValueChange={setSwitcherChoice}>
                  <SelectTrigger id="add-switcher" className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {switchers.data.map((candidate) => (
                      <SelectItem key={candidate.id} value={candidate.id}>
                        {candidate.name}
                        {candidate.paused && t` (paused)`}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              ) : (
                <p className="text-sm text-muted-foreground">
                  {switchers.error
                    ? errorMessage(switchers.error)
                    : t`This workspace has no switchers yet.`}
                </p>
              )}
            </div>
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="add-name">
              <Trans>Name</Trans>
            </Label>
            <Input
              id="add-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              maxLength={60}
            />
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="add-stats-url">
              <Trans>Stats URL</Trans>
            </Label>
            <SecretUrlInput
              id="add-stats-url"
              value={statsUrl}
              onChange={setStatsUrl}
              label={t`stats URL`}
            />
          </div>

          <ServerDetailsFields
            kind={request.kind}
            form={details}
            onChange={setDetails}
            idPrefix="add"
          />

          {create.error && <p className="text-sm text-destructive">{errorMessage(create.error)}</p>}

          <div className="grid grid-cols-2 gap-2.5">
            <Button asChild variant="outline" className="h-12 rounded-xl">
              <Link to="/" onClick={clearAddServer}>
                <Trans>Cancel</Trans>
              </Link>
            </Button>
            <Button
              type="submit"
              className="h-12 rounded-xl"
              disabled={!switcher || !name.trim() || !statsUrl.trim() || create.isPending}
            >
              {create.isPending ? t`Adding…` : t`Add server`}
            </Button>
          </div>
        </>
      )}
    </form>
  );
}
