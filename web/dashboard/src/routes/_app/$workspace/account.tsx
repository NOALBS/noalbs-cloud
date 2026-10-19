import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { Download, Monitor, Smartphone } from "lucide-react";
import { useEffect, useRef } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { providersQuery, queryKeys, sessionsQuery } from "@/api/queries";
import type { Connection, Session } from "@/api/types";
import { Page, Section } from "@/components/page";
import { TypeToConfirm } from "@/components/type-to-confirm";
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
import { Skeleton } from "@/components/ui/skeleton";
import { providerLabel, UserAvatar } from "@/components/user-avatar";
import { useWorkspace } from "@/hooks/use-workspace";
import { downloadText } from "@/lib/download";
import { errorMessage } from "@/lib/errors";
import { PLATFORMS } from "@/lib/platforms";
import { formatDate, formatRelative } from "@/lib/time";
import { describeUserAgent } from "@/lib/user-agent";

interface AccountSearch {
  /** Set by the API after connecting an account: the platform, e.g. `kick`. */
  linked?: string;
  /** Set by the API when connecting failed: an error code. */
  link_error?: string;
}

export const Route = createFileRoute("/_app/$workspace/account")({
  validateSearch: (search: Record<string, unknown>): AccountSearch => ({
    linked: typeof search.linked === "string" ? search.linked : undefined,
    link_error: typeof search.link_error === "string" ? search.link_error : undefined,
  }),
  component: AccountPage,
});

const LINK_ERRORS: Record<string, string> = {
  account_already_linked:
    "That account is already linked to another NOALBS user. Sign in with it and disconnect it there first.",
  oauth_denied: "Connecting was cancelled.",
  no_youtube_channel:
    "That Google account has no YouTube channel. Pick the account (or brand account) you stream with.",
};

function AccountPage() {
  const { me } = useWorkspace();
  const search = Route.useSearch();
  const navigate = useNavigate();
  const shown = useRef(false);

  // Say how connecting went (the API sends us back here), then tidy the URL.
  useEffect(() => {
    if (shown.current || (!search.linked && !search.link_error)) return;
    shown.current = true;
    if (search.linked) {
      const platform = PLATFORMS.find((item) => item.path === search.linked);
      toast.success(`${platform?.label ?? "Account"} connected`);
    } else if (search.link_error) {
      toast.error(
        LINK_ERRORS[search.link_error] ?? "Couldn’t connect the account. Please try again.",
      );
    }
    void navigate({ to: ".", search: {}, replace: true });
  }, [search.linked, search.link_error, navigate]);

  return (
    <Page title="Account" description="How you sign in, and where you’re signed in.">
      <ConnectedAccounts connections={me.connections} />
      <Sessions />
      <YourData username={me.connections[0]?.username ?? "delete"} />
    </Page>
  );
}

/** Download a copy of your data, or delete your account (GDPR). */
function YourData({ username }: { username: string }) {
  const queryClient = useQueryClient();
  const download = useMutation({
    mutationFn: () => unwrap(api.GET("/me/export")),
    onSuccess: (data) => {
      downloadText("noalbs-cloud-data.json", JSON.stringify(data, null, 2), "application/json");
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title="Your data"
      description={
        <>
          What we keep and why is in the{" "}
          <a href="/privacy" className="font-medium text-primary hover:underline">
            privacy policy
          </a>
          ; the rules for using NOALBS Cloud are in the{" "}
          <a href="/terms" className="font-medium text-primary hover:underline">
            terms of service
          </a>
          .
        </>
      }
    >
      <div className="flex flex-col gap-5 px-5 pb-5">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <span className="flex flex-col gap-0.5">
            <span className="text-sm font-semibold">Download my data</span>
            <span className="text-sm text-muted-foreground">
              Your account, sign-ins, workspaces, switchers and their settings, and what you did in
              teams and chat, as a JSON file. It holds your stats URLs and stream keys: keep it
              private.
            </span>
          </span>
          <Button variant="outline" disabled={download.isPending} onClick={() => download.mutate()}>
            <Download />
            Download
          </Button>
        </div>
        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-divider pt-5">
          <span className="flex flex-col gap-0.5">
            <span className="text-sm font-semibold">Download stream history</span>
            <span className="text-sm text-muted-foreground">
              The stream history, stats, markers and activity log of your own switchers, one JSON
              object per line. It can be large.
            </span>
          </span>
          <Button variant="outline" asChild>
            <a href="/api/me/export/history" download>
              <Download />
              Download
            </a>
          </Button>
        </div>
        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-divider pt-5">
          <span className="flex flex-col gap-0.5">
            <span className="text-sm font-semibold">Delete account</span>
            <span className="text-sm text-muted-foreground">
              Removes your account and every workspace you own on your own. This can’t be undone.
            </span>
          </span>
          <TypeToConfirm
            trigger="Delete account"
            title="Delete your account?"
            description={
              <>
                <p>
                  Your account, sign-ins and memberships are deleted. Workspaces you own on your own
                  go too, with their switchers and stream history, and their subscriptions end
                  straight away without a refund.
                </p>
                <p>
                  If you own a workspace with other members, transfer it to an admin, remove them or
                  delete that workspace first.
                </p>
              </>
            }
            phrase={username}
            confirmLabel="Delete my account"
            onConfirm={async () => {
              await unwrap(api.DELETE("/me"));
              queryClient.clear();
              window.location.assign("/");
            }}
          />
        </div>
      </div>
    </Section>
  );
}

function ConnectedAccounts({ connections }: { connections: Connection[] }) {
  const { workspace } = useWorkspace();
  const providers = useQuery(providersQuery);
  const returnTo = `/${workspace.url_slug}/account`;
  const others = connections.filter(
    (connection) => !PLATFORMS.some((platform) => platform.provider === connection.provider),
  );

  return (
    <Section
      title="Connected accounts"
      description="Sign in with any of these. Connect both to keep a way in if one has trouble."
    >
      <ul>
        {PLATFORMS.map((platform) => {
          const linked = connections.filter((item) => item.provider === platform.provider);
          const available = providers.data?.[platform.path] ?? false;
          return (
            <li key={platform.provider} className="border-t border-divider">
              {linked.map((connection) => (
                <ConnectionRow
                  key={connection.id}
                  connection={connection}
                  onlyOne={connections.length === 1}
                />
              ))}
              {linked.length === 0 && (
                <div className="flex min-h-16 flex-wrap items-center gap-3 px-5 py-3">
                  <span className="flex flex-1 flex-col gap-0.5">
                    <span className="font-semibold">{platform.label}</span>
                    <span className="text-sm text-muted-foreground">Not connected</span>
                  </span>
                  {available ? (
                    <a
                      href={`/api/auth/link/${platform.path}?redirect=${encodeURIComponent(returnTo)}`}
                      className={`flex h-9 items-center rounded-lg px-3.5 text-sm font-semibold ${platform.button}`}
                    >
                      Connect {platform.label}
                    </a>
                  ) : (
                    <span className="text-sm text-muted-foreground">
                      {providers.isPending ? "…" : "Not available on this server"}
                    </span>
                  )}
                </div>
              )}
            </li>
          );
        })}
        {others.map((connection) => (
          <li key={connection.id} className="border-t border-divider">
            <ConnectionRow connection={connection} onlyOne={connections.length === 1} />
          </li>
        ))}
      </ul>
    </Section>
  );
}

function ConnectionRow({ connection, onlyOne }: { connection: Connection; onlyOne: boolean }) {
  const queryClient = useQueryClient();
  const label = providerLabel(connection.provider);

  const disconnect = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE("/me/connections/{connection_id}", {
          params: { path: { connection_id: connection.id } },
        }),
      ),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.me }),
        queryClient.invalidateQueries({ queryKey: queryKeys.sessions }),
      ]);
      toast.success(`${label} disconnected`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <div className="flex min-h-16 flex-wrap items-center gap-3 px-5 py-3">
      <UserAvatar name={connection.username} avatarUrl={connection.avatar_url} />
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="flex items-center gap-2 font-semibold">
          {label}
          <span className="truncate font-normal text-muted-foreground">{connection.username}</span>
        </span>
        <span className="text-sm text-muted-foreground">
          Connected {formatDate(connection.connected_at)}
          {onlyOne && " · your only way to sign in"}
        </span>
      </span>
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button variant="outline" size="sm" disabled={onlyOne || disconnect.isPending}>
            Disconnect
          </Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              Disconnect {label} ({connection.username})?
            </AlertDialogTitle>
            <AlertDialogDescription>
              You won’t be able to sign in with it anymore. Other browsers signed in with it are
              signed out; this one stays signed in.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={() => disconnect.mutate()}>
              Disconnect
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function Sessions() {
  const queryClient = useQueryClient();
  const sessions = useQuery(sessionsQuery);
  const others = sessions.data?.filter((session) => !session.current) ?? [];

  const signOutOthers = useMutation({
    mutationFn: () => unwrap(api.POST("/me/sessions/sign-out-others")),
    onSuccess: async ({ signed_out }) => {
      await queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
      toast.success(
        signed_out === 1 ? "Signed out 1 other session" : `Signed out ${signed_out} sessions`,
      );
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title="Sessions"
      description="Browsers signed in to your account. Sign out any you don’t recognise."
      actions={
        others.length > 0 && (
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button variant="outline" size="sm" disabled={signOutOthers.isPending}>
                Sign out other sessions
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Sign out everywhere else?</AlertDialogTitle>
                <AlertDialogDescription>
                  {others.length === 1
                    ? "1 other browser will need to sign in again."
                    : `${others.length} other browsers will need to sign in again.`}{" "}
                  This one stays signed in.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction variant="destructive" onClick={() => signOutOthers.mutate()}>
                  Sign out others
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        )
      }
    >
      {sessions.isPending && <Skeleton className="mx-5 mb-5 h-16 rounded-xl" />}
      {sessions.isError && (
        <p className="px-5 pb-5 text-sm text-destructive">{errorMessage(sessions.error)}</p>
      )}
      <ul aria-label="Sessions">
        {sessions.data?.map((session) => (
          <SessionRow key={session.id} session={session} />
        ))}
      </ul>
    </Section>
  );
}

function SessionRow({ session }: { session: Session }) {
  const queryClient = useQueryClient();
  const device = describeUserAgent(session.user_agent);
  const Icon = device.mobile ? Smartphone : Monitor;

  const signOut = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE("/me/sessions/{session_id}", {
          params: { path: { session_id: session.id } },
        }),
      ),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: queryKeys.sessions });
      toast.success(`Signed out ${device.label}`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <li className="flex min-h-16 flex-wrap items-center gap-3 border-t border-divider px-5 py-3">
      <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-surface-2 text-muted-foreground">
        <Icon className="size-4.5" />
      </span>
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="flex items-center gap-2 font-semibold">
          {device.label}
          {session.current && <Badge variant="secondary">This browser</Badge>}
        </span>
        <span className="text-sm text-muted-foreground">
          {providerLabel(session.provider)} ·{" "}
          {session.current ? "Active now" : `Active ${formatRelative(session.last_used_at)}`} ·
          Signed in {formatDate(session.created_at)}
        </span>
      </span>
      {!session.current && (
        <Button
          variant="outline"
          size="sm"
          onClick={() => signOut.mutate()}
          disabled={signOut.isPending}
        >
          Sign out
        </Button>
      )}
    </li>
  );
}
