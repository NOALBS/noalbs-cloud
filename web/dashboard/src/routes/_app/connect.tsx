import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient, useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import { ApiRequestError, api, unwrap } from "@/api/client";
import {
  connectPairingQuery,
  meQuery,
  queryKeys,
  switchersQuery,
  workspacesQuery,
} from "@/api/queries";
import { MinimalHeader } from "@/components/minimal-header";
import { NoticeBanners } from "@/components/notice-banner";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { errorMessage } from "@/lib/errors";
import { actorOf, can, isSupport } from "@/lib/permissions";

interface ConnectSearch {
  code?: string;
}

export const Route = createFileRoute("/_app/connect")({
  validateSearch: (search: Record<string, unknown>): ConnectSearch => ({
    code: typeof search.code === "string" && search.code ? search.code : undefined,
  }),
  loader: ({ context }) => context.queryClient.ensureQueryData(workspacesQuery),
  component: ConnectPairingPage,
});

function ConnectPairingPage() {
  const { code } = Route.useSearch();
  const { data: me } = useSuspenseQuery(meQuery);
  const { data: workspaces } = useSuspenseQuery(workspacesQuery);
  const preview = useQuery({
    ...connectPairingQuery(code ?? ""),
    enabled: Boolean(code),
  });
  const queryClient = useQueryClient();
  const [workspaceChoice, setWorkspaceChoice] = useState<string | null>(null);
  const [switcherChoice, setSwitcherChoice] = useState<string | null>(null);
  const [confirmed, setConfirmed] = useState(false);

  const eligible = workspaces.filter((workspace) => {
    const actor = actorOf(workspace, me);
    return !isSupport(actor) && can(actor, "switcher.manage");
  });
  const workspace =
    eligible.find((candidate) => candidate.id === workspaceChoice) ??
    eligible.find((candidate) => candidate.url_slug === me.last_workspace_slug) ??
    eligible[0];
  const switchers = useQuery({
    ...switchersQuery(workspace?.id ?? ""),
    enabled: Boolean(workspace),
  });
  const switcher =
    switchers.data?.find((candidate) => candidate.id === switcherChoice) ?? switchers.data?.[0];

  const confirm = useMutation({
    mutationFn: () => {
      if (!workspace || !switcher || !code) throw new Error("Select a switcher");
      return unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/connect/pair", {
          params: { path: { workspace_id: workspace.id, switcher_id: switcher.id } },
          body: { user_code: code },
        }),
      );
    },
    onSuccess: async () => {
      setConfirmed(true);
      await queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
    },
  });

  const expired = preview.error instanceof ApiRequestError && preview.error.status === 404;

  return (
    <div className="flex min-h-dvh flex-col">
      <MinimalHeader me={me} />
      <NoticeBanners />
      <main className="flex flex-1 items-center justify-center px-4 py-10">
        <section className="flex w-full max-w-xl flex-col gap-5 rounded-3xl border bg-card p-6 md:p-8">
          <div className="flex flex-col gap-1">
            <span className="text-sm text-muted-foreground">
              <Trans>NOALBS Cloud Connect</Trans>
            </span>
            <h1 className="text-2xl font-bold">
              {confirmed ? t`Computer connected` : t`Connect this computer`}
            </h1>
          </div>

          {!code && (
            <PairingProblem
              title={t`This verification link is incomplete`}
              detail={t`Return to the NOALBS Cloud Connect app and open its pairing link again.`}
            />
          )}
          {code && preview.isPending && (
            <p className="text-sm text-muted-foreground">
              <Trans>Checking this pairing code…</Trans>
            </p>
          )}
          {code && preview.error && (
            <PairingProblem
              title={expired ? t`This pairing code has expired` : t`Couldn’t check this code`}
              detail={
                expired
                  ? t`Return to the NOALBS Cloud Connect app to start a new pairing.`
                  : errorMessage(preview.error)
              }
            />
          )}
          {confirmed && (
            <p className="rounded-2xl border border-live/30 bg-live/5 p-4 text-sm">
              <Trans>You can close this tab; Connect is now connected.</Trans>
            </p>
          )}
          {code && preview.data && !confirmed && (
            <>
              <div className="rounded-2xl border border-divider bg-sunken p-4">
                <p className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">
                  <Trans>Computer requesting access</Trans>
                </p>
                <p className="mt-1 text-lg font-semibold">{preview.data.name}</p>
                <p className="text-sm text-muted-foreground">
                  {preview.data.os} · {preview.data.app_version}
                </p>
                <p className="mt-2 font-mono text-sm tracking-widest">{code}</p>
              </div>

              {eligible.length === 0 ? (
                <PairingProblem
                  title={t`No switcher you can manage`}
                  detail={t`You need switcher management access in a workspace to pair this computer.`}
                />
              ) : (
                <>
                  <div className="flex flex-col gap-2">
                    <label htmlFor="connect-workspace" className="text-sm font-medium">
                      <Trans>Workspace</Trans>
                    </label>
                    <Select
                      value={workspace?.id}
                      onValueChange={(id) => {
                        setWorkspaceChoice(id);
                        setSwitcherChoice(null);
                      }}
                    >
                      <SelectTrigger id="connect-workspace" className="w-full">
                        <SelectValue placeholder={t`Choose a workspace`} />
                      </SelectTrigger>
                      <SelectContent>
                        {eligible.map((option) => (
                          <SelectItem key={option.id} value={option.id}>
                            {option.name}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>

                  <div className="flex flex-col gap-2">
                    <label htmlFor="connect-switcher" className="text-sm font-medium">
                      <Trans>Switcher</Trans>
                    </label>
                    <Select
                      value={switcher?.id}
                      onValueChange={setSwitcherChoice}
                      disabled={switchers.isPending || !switchers.data?.length}
                    >
                      <SelectTrigger id="connect-switcher" className="w-full">
                        <SelectValue placeholder={t`Choose a switcher`} />
                      </SelectTrigger>
                      <SelectContent>
                        {switchers.data?.map((option) => (
                          <SelectItem key={option.id} value={option.id}>
                            {option.name}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    {switchers.error && (
                      <p className="text-sm text-destructive">{errorMessage(switchers.error)}</p>
                    )}
                  </div>

                  {confirm.error && (
                    <p className="text-sm text-destructive">
                      {confirm.error instanceof ApiRequestError && confirm.error.status === 404
                        ? t`This pairing code expired or was already confirmed. Return to the app to start again.`
                        : errorMessage(confirm.error)}
                    </p>
                  )}
                  <Button
                    className="h-11 rounded-xl"
                    onClick={() => confirm.mutate()}
                    disabled={!switcher || confirm.isPending}
                  >
                    {confirm.isPending ? t`Connecting…` : t`Connect to this switcher`}
                  </Button>
                </>
              )}
            </>
          )}
        </section>
      </main>
    </div>
  );
}

function PairingProblem({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="flex flex-col gap-1 rounded-2xl border border-divider p-4">
      <p className="font-semibold">{title}</p>
      <p className="text-sm text-muted-foreground">{detail}</p>
    </div>
  );
}
