import { plural, t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { Check } from "lucide-react";
import type { ReactNode } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys, streamServersQuery } from "@/api/queries";
import { BelaboxSettings } from "@/components/belabox-settings";
import { CopyField } from "@/components/copy-field";
import { DocsLink } from "@/components/docs-link";
import { MoblinSettings } from "@/components/moblin-settings";
import { ObsConnectionForm } from "@/components/obs-connection-form";
import { ReadOnlyNotice } from "@/components/settings";
import { SwitcherPage } from "@/components/switcher-page";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { useSwitcher } from "@/hooks/use-switcher";
import { type BelaboxView, belaboxView } from "@/lib/belabox";
import { errorMessage } from "@/lib/errors";
import { type MoblinView, moblinView } from "@/lib/moblin";
import { setupProgress } from "@/lib/setup";
import { cn } from "@/lib/utils";
import { isStale } from "@/live/reducer";
import { useSwitcherLive } from "@/live/use-switcher-live";

export const Route = createFileRoute("/_app/$workspace/s/$switcher/setup")({
  component: SetupPage,
});

function SetupPage() {
  const { workspace, switcher, canManage, path } = useSwitcher();
  const servers = useQuery(streamServersQuery(workspace.id, switcher.id));
  const params = { workspace: workspace.url_slug, switcher: switcher.id };

  const feed = useSwitcherLive(workspace.id, switcher.id);
  const status = feed.status && !isStale(feed.status) ? feed.status : null;
  // Done once it has ever connected; the live status shows what's up right now.
  const { connected, hasServer, scenesSet, done } = setupProgress({
    connected: Boolean(status?.obs_connected) || switcher.last_seen_at != null,
    serverCount: servers.data?.length ?? 0,
    scenes: switcher.scenes,
  });

  return (
    <SwitcherPage
      title={t`Setup`}
      description={t`Connect OBS and your stream server to ${switcher.name}, and optionally your BELABOX or Moblin.`}
      backToSettings
      className="max-w-3xl"
    >
      <div className="flex flex-col gap-2">
        <div className="flex justify-between text-sm text-muted-foreground">
          <span>{done === 3 ? t`All set` : t`${done} of 3 done`}</span>
          <span className="font-mono tabular">{Math.round((done / 3) * 100)}%</span>
        </div>
        <Progress value={(done / 3) * 100} aria-label={t`Setup progress`} />
        {done < 3 && (
          <p className="text-xs text-muted-foreground">
            <Trans>
              Stuck on a step? The{" "}
              <DocsLink page="getting-started/quick-start">quick start guide</DocsLink> walks
              through it, and{" "}
              <DocsLink page="getting-started/introduction">how NOALBS connects</DocsLink> explains
              where your video goes.
            </Trans>
          </p>
        )}
      </div>

      {done === 3 && (
        <section className="flex flex-col gap-3 rounded-2xl border-2 border-live/40 bg-live/5 p-5">
          <div className="flex flex-col gap-0.5">
            <h2 className="font-semibold">
              <Trans>Try it before you go live</Trans>
            </h2>
            <p className="text-sm text-muted-foreground">
              <Trans>
                {switcher.behaviour.only_switch_when_streaming
                  ? t`NOALBS only switches while OBS is streaming (you can change that under Switching), so start the stream in OBS first. Then turn off your encoder`
                  : t`Turn off your encoder`}
                : within a few seconds OBS should go to your BRB scene, and back to LIVE when the
                encoder is on again.
              </Trans>
            </p>
          </div>
          <Button asChild className="self-start">
            <Link to="/$workspace/s/$switcher/live" params={params}>
              <Trans>Open the Live page</Trans>
            </Link>
          </Button>
        </section>
      )}

      {!canManage && (
        <ReadOnlyNotice>
          <Trans>Only admins can change the setup of a switcher.</Trans>
        </ReadOnlyNotice>
      )}

      <Step
        number={1}
        done={connected}
        active={!connected}
        title={t`Connect OBS`}
        description={t`A browser source in OBS links it to this switcher, with no ports to open.`}
      >
        <BrowserSourceStep
          canManage={canManage}
          path={path}
          connected={connected}
          browserSource={Boolean(status?.browser_source)}
          obs={Boolean(status?.obs_connected)}
        />
      </Step>

      <Step
        number={2}
        done={hasServer}
        active={connected && !hasServer}
        title={t`Connect your stream server`}
        description={t`Where your encoder sends video. NOALBS reads its stats to pick a scene.`}
      >
        {hasServer ? (
          <p className="text-sm text-muted-foreground">
            {servers.data?.length === 1
              ? t`Using ${servers.data[0]?.name ?? ""}.`
              : plural(servers.data?.length ?? 0, {
                  one: "# stream server, tried in order.",
                  other: "# stream servers, tried in order.",
                })}{" "}
            <Link
              to="/$workspace/s/$switcher/switching"
              hash="servers"
              params={params}
              className="font-medium text-primary hover:underline"
            >
              <Trans>Manage stream servers</Trans>
            </Link>
          </p>
        ) : (
          <Button asChild className="self-start">
            <Link to="/$workspace/s/$switcher/switching" hash="servers" params={params}>
              <Trans>Add a stream server</Trans>
            </Link>
          </Button>
        )}
      </Step>

      <Step
        number={3}
        done={scenesSet}
        active={connected && hasServer && !scenesSet}
        title={t`Pick your LIVE, LOW and BRB scenes`}
        description={t`The scenes NOALBS switches between. They’re loaded from OBS once it’s connected.`}
      >
        <Link
          to="/$workspace/s/$switcher/switching"
          hash="scenes"
          params={params}
          className="text-sm font-medium text-primary hover:underline"
        >
          <Trans>Go to scenes</Trans>
        </Link>
      </Step>

      <Step
        number={4}
        done={switcher.belabox.has_remote_key}
        active={false}
        optional
        title={t`Connect BELABOX Cloud`}
        description={t`Using a BELABOX? See its modems on the Live page, control it from the dashboard and chat, and start or stop it with the OBS stream.`}
      >
        <div className="rounded-xl border border-divider pt-4">
          <BelaboxSettings />
        </div>
        {switcher.belabox.has_remote_key && (
          <ul className="flex flex-col gap-1.5 rounded-xl border border-divider bg-sunken px-3.5 py-3 text-sm">
            <BelaboxLine view={belaboxView(feed.belabox, Date.now())} />
          </ul>
        )}
      </Step>

      <Step
        number={5}
        done={switcher.moblin.has_password}
        active={false}
        optional
        title={t`Connect Moblin`}
        description={t`Streaming with Moblin? See its bitrate, bonding and battery on the Live page, control it from the dashboard and chat, and start or stop it with the OBS stream.`}
      >
        <div className="rounded-xl border border-divider pt-4">
          <MoblinSettings />
        </div>
        {switcher.moblin.has_password && (
          <ul className="flex flex-col gap-1.5 rounded-xl border border-divider bg-sunken px-3.5 py-3 text-sm">
            <MoblinLine view={moblinView(feed.moblin, Date.now())} />
          </ul>
        )}
      </Step>
    </SwitcherPage>
  );
}

/** Whether the BELABOX link is up, for the optional BELABOX step. */
function BelaboxLine({ view }: { view: BelaboxView }) {
  switch (view.kind) {
    case "online":
      return <ConnectionLine ok done={t`BELABOX connected`} waiting="" />;
    case "invalid_key":
      return (
        <ConnectionLine
          ok={false}
          done=""
          waiting={t`BELABOX Cloud refused the secret URL. Paste it again above.`}
        />
      );
    case "offline":
      return (
        <ConnectionLine
          ok={false}
          done=""
          waiting={t`Connected to BELABOX Cloud; waiting for your BELABOX to come online…`}
        />
      );
    case "idle":
      return (
        <ConnectionLine
          ok={false}
          done=""
          waiting={t`BELABOX connects while OBS is open with the browser source.`}
        />
      );
    case "loading":
    case "connecting":
      return <ConnectionLine ok={false} done="" waiting={t`Connecting to BELABOX Cloud…`} />;
  }
}

/** Whether Moblin is connected, for the optional Moblin step. */
function MoblinLine({ view }: { view: MoblinView }) {
  switch (view.kind) {
    case "connected":
    case "paused":
      return <ConnectionLine ok done={t`Moblin connected`} waiting="" />;
    case "loading":
    case "disconnected":
      return (
        <ConnectionLine
          ok={false}
          done=""
          waiting={t`Waiting for Moblin… Enter the password and URL above, then turn the streamer on.`}
        />
      );
  }
}

function Step({
  number,
  done,
  active,
  optional = false,
  title,
  description,
  children,
}: {
  number: number;
  done: boolean;
  active: boolean;
  /** Not needed for switching; doesn't count toward the setup progress. */
  optional?: boolean;
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <section
      className={cn(
        "flex flex-col gap-4 rounded-2xl p-5",
        active ? "border-2 border-primary bg-card" : "border bg-card",
      )}
    >
      <div className="flex items-start gap-3">
        <span
          className={cn(
            "flex size-7 shrink-0 items-center justify-center rounded-full font-mono text-sm font-bold",
            done && "bg-live/15 text-live",
            !done && active && "bg-primary text-primary-foreground",
            !done && !active && "border text-muted-foreground",
          )}
        >
          {done ? <Check className="size-4" /> : number}
        </span>
        <div className="flex flex-col gap-0.5">
          <h2 className="flex flex-wrap items-center gap-2 font-semibold">
            {title}
            {optional && (
              <Badge variant="secondary" className="font-normal">
                <Trans>Optional</Trans>
              </Badge>
            )}
          </h2>
          <p className="text-sm text-muted-foreground">{description}</p>
        </div>
      </div>
      <div className="flex flex-col gap-3 sm:pl-10">{children}</div>
    </section>
  );
}

function BrowserSourceStep({
  canManage,
  path,
  connected,
  browserSource,
  obs,
}: {
  canManage: boolean;
  path: { workspace_id: string; switcher_id: string };
  connected: boolean;
  /** Live: the browser source is connected to NOALBS right now. */
  browserSource: boolean;
  /** Live: the browser source reached OBS. */
  obs: boolean;
}) {
  const queryClient = useQueryClient();
  // Only known right after the switcher (or a new link) was created.
  const { data: url } = useQuery<string | null>({
    queryKey: queryKeys.connectLink(path.switcher_id),
    queryFn: () => null,
    staleTime: Number.POSITIVE_INFINITY,
  });

  const rotate = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/connect-token", {
          params: { path },
        }),
      ),
    onSuccess: (link) => {
      queryClient.setQueryData(queryKeys.connectLink(path.switcher_id), link.browser_source_url);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <>
      <ol className="flex list-decimal flex-col gap-1.5 pl-5 text-sm text-muted-foreground">
        <li>
          <Trans>
            In OBS, open{" "}
            <strong className="text-foreground">Tools → WebSocket Server Settings</strong>, turn on
            the WebSocket server and enter its port and password here (
            <strong className="text-foreground">Show Connect Info</strong> shows both).
          </Trans>
        </li>
      </ol>
      <ObsConnectionForm className="rounded-xl border border-divider p-4" />
      <ol
        start={2}
        className="flex list-decimal flex-col gap-1.5 pl-5 text-sm text-muted-foreground"
      >
        <li>
          <Trans>
            Add a <strong className="text-foreground">Browser</strong> source to a scene you always
            have open (it doesn’t have to be visible), paste the link below as its URL, and keep it
            enabled. Leave{" "}
            <strong className="text-foreground">Shutdown source when not visible</strong> off, or
            NOALBS stops whenever that scene isn’t on air.
          </Trans>
        </li>
      </ol>

      {url ? (
        <div className="flex flex-col gap-2">
          <CopyField value={url} label={t`browser source URL`} secret />
          <p className="text-xs text-muted-foreground">
            <Trans>
              Copy it now: the link contains a secret, so it won’t be shown again. You can always
              create a new one.
            </Trans>
          </p>
        </div>
      ) : canManage ? (
        connected ? (
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button variant="outline" className="self-start">
                <Trans>Create a new link</Trans>
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>
                  <Trans>Create a new browser-source link?</Trans>
                </AlertDialogTitle>
                <AlertDialogDescription>
                  <Trans>
                    The current link stops working, so you’ll need to paste the new one into OBS.
                  </Trans>
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>
                  <Trans>Cancel</Trans>
                </AlertDialogCancel>
                <AlertDialogAction onClick={() => rotate.mutate()}>
                  <Trans>Create new link</Trans>
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        ) : (
          <div className="flex flex-col gap-1.5">
            <Button
              className="self-start"
              onClick={() => rotate.mutate()}
              disabled={rotate.isPending}
            >
              <Trans>Create browser-source link</Trans>
            </Button>
            <p className="text-xs text-muted-foreground">
              <Trans>If you already pasted a link into OBS, this replaces it.</Trans>
            </p>
          </div>
        )
      ) : null}

      <ul className="flex flex-col gap-1.5 rounded-xl border border-divider bg-sunken px-3.5 py-3 text-sm">
        <ConnectionLine
          ok={browserSource}
          done={t`Browser source connected`}
          waiting={t`Waiting for the browser source…`}
        />
        <ConnectionLine
          ok={obs}
          done={t`OBS connected`}
          waiting={
            browserSource
              ? t`Browser source is up; waiting for OBS. Check the WebSocket port and password above.`
              : t`OBS connects through the browser source`
          }
        />
      </ul>
      {!obs && (
        <p className="text-xs text-muted-foreground">
          <Trans>Not connecting?</Trans>{" "}
          <DocsLink
            page="guides/troubleshooting"
            anchor={
              browserSource
                ? "browser-source-is-up-waiting-for-obs"
                : "waiting-for-the-browser-source"
            }
          >
            <Trans>See troubleshooting</Trans>
          </DocsLink>
          .
        </p>
      )}
    </>
  );
}

function ConnectionLine({ ok, done, waiting }: { ok: boolean; done: string; waiting: string }) {
  return (
    <li className={cn("flex items-start gap-2", ok ? "text-foreground" : "text-muted-foreground")}>
      <span
        aria-hidden="true"
        className={cn(
          "mt-1.5 size-2 shrink-0 rounded-full",
          ok ? "bg-live" : "animate-pulse bg-low",
        )}
      />
      {ok ? done : waiting}
    </li>
  );
}
