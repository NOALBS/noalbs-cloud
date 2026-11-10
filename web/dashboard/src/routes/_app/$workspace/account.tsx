import { plural, t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { Download, Monitor, Smartphone } from "lucide-react";
import { useEffect, useRef } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { providersQuery, queryKeys, sessionsQuery } from "@/api/queries";
import type { Connection, Session } from "@/api/types";
import { Page, Section } from "@/components/page";
import { Row, RowList } from "@/components/row-list";
import { SettingsRow } from "@/components/settings-row";
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

const linkErrors = (): Record<string, string> => ({
  account_already_linked: t`That account is already linked to another NOALBS user. Sign in with it and disconnect it there first.`,
  oauth_denied: t`Connecting was cancelled.`,
  no_youtube_channel: t`That Google account has no YouTube channel. Pick the account (or brand account) you stream with.`,
});

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
      toast.success(t`${platform?.label ?? t`Account`} connected`);
    } else if (search.link_error) {
      toast.error(
        linkErrors()[search.link_error] ?? t`Couldn’t connect the account. Please try again.`,
      );
    }
    void navigate({ to: ".", search: {}, replace: true });
  }, [search.linked, search.link_error, navigate]);

  return (
    <Page title={t`Account`} description={t`How you sign in, and where you’re signed in.`}>
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
    <>
      <Section
        title={t`Your data`}
        description={
          <>
            <Trans>
              What we keep and why is in the{" "}
              <a href="/privacy" className="font-medium text-primary hover:underline">
                privacy policy
              </a>
              ; the rules for using NOALBS Cloud are in the{" "}
              <a href="/terms" className="font-medium text-primary hover:underline">
                terms of service
              </a>
              .
            </Trans>
          </>
        }
      >
        <SettingsRow
          label={t`Download my data`}
          description={
            <Trans>
              Your account, sign-ins, workspaces, switchers and their settings, and what you did in
              teams and chat, as a JSON file. It holds your stats URLs and stream keys: keep it
              private.
            </Trans>
          }
        >
          <Button variant="outline" disabled={download.isPending} onClick={() => download.mutate()}>
            <Download />
            <Trans>Download</Trans>
          </Button>
        </SettingsRow>
        <SettingsRow
          label={t`Download stream history`}
          description={
            <Trans>
              The stream history, stats, markers and activity log of your own switchers, one JSON
              object per line. It can be large.
            </Trans>
          }
        >
          <Button variant="outline" asChild>
            <a href="/api/me/export/history" download>
              <Download />
              <Trans>Download</Trans>
            </a>
          </Button>
        </SettingsRow>
      </Section>
      <Section title={t`Delete account`}>
        <SettingsRow
          label={t`Delete account`}
          description={
            <Trans>
              Removes your account and every workspace you own on your own. This can’t be undone.
            </Trans>
          }
        >
          <TypeToConfirm
            trigger={t`Delete account`}
            title={t`Delete your account?`}
            description={
              <>
                <p>
                  <Trans>
                    Your account, sign-ins and memberships are deleted. Workspaces you own on your
                    own go too, with their switchers and stream history, and their subscriptions end
                    straight away without a refund.
                  </Trans>
                </p>
                <p>
                  <Trans>
                    If you own a workspace with other members, transfer it to an admin, remove them
                    or delete that workspace first.
                  </Trans>
                </p>
              </>
            }
            phrase={username}
            confirmLabel={t`Delete my account`}
            onConfirm={async () => {
              await unwrap(api.DELETE("/me"));
              queryClient.clear();
              window.location.assign("/");
            }}
          />
        </SettingsRow>
      </Section>
    </>
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
      title={t`Connected accounts`}
      description={t`Sign in with any of these. Connect both to keep a way in if one has trouble.`}
    >
      <RowList>
        {PLATFORMS.flatMap((platform) => {
          const linked = connections.filter((item) => item.provider === platform.provider);
          const available = providers.data?.[platform.path] ?? false;
          return linked.length > 0 ? (
            linked.map((connection) => (
              <ConnectionRow
                key={connection.id}
                connection={connection}
                onlyOne={connections.length === 1}
              />
            ))
          ) : (
            <Row key={platform.provider} title={platform.label} meta={t`Not connected`}>
              {available ? (
                <a
                  href={`/api/auth/link/${platform.path}?redirect=${encodeURIComponent(returnTo)}`}
                  className="flex h-9 items-center rounded-lg border px-3.5 text-sm font-semibold hover:bg-accent"
                >
                  <Trans>Connect {platform.label}</Trans>
                </a>
              ) : (
                <span className="text-xs text-muted-foreground">
                  {providers.isPending ? "…" : t`Not available on this server`}
                </span>
              )}
            </Row>
          );
        })}
        {others.map((connection) => (
          <ConnectionRow
            key={connection.id}
            connection={connection}
            onlyOne={connections.length === 1}
          />
        ))}
      </RowList>
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
      toast.success(t`${label} disconnected`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Row
      lead={<UserAvatar name={connection.username} avatarUrl={connection.avatar_url} />}
      title={
        <span className="flex flex-wrap items-center gap-2">
          {label}
          <span className="truncate font-normal text-muted-foreground">{connection.username}</span>
        </span>
      }
      meta={
        <Trans>
          Connected {formatDate(connection.connected_at)}
          {onlyOne && t` · your only way to sign in`}
        </Trans>
      }
    >
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button variant="outline" size="sm" disabled={onlyOne || disconnect.isPending}>
            <Trans>Disconnect</Trans>
          </Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              <Trans>
                Disconnect {label} ({connection.username})?
              </Trans>
            </AlertDialogTitle>
            <AlertDialogDescription>
              <Trans>
                You won’t be able to sign in with it anymore. Other browsers signed in with it are
                signed out; this one stays signed in.
              </Trans>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>
              <Trans>Cancel</Trans>
            </AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={() => disconnect.mutate()}>
              <Trans>Disconnect</Trans>
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Row>
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
        plural(signed_out, {
          one: "Signed out # other session",
          other: "Signed out # sessions",
        }),
      );
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title={t`Sessions`}
      description={t`Browsers signed in to your account. Sign out any you don’t recognise.`}
      actions={
        others.length > 0 && (
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button variant="outline" size="sm" disabled={signOutOthers.isPending}>
                <Trans>Sign out other sessions</Trans>
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>
                  <Trans>Sign out everywhere else?</Trans>
                </AlertDialogTitle>
                <AlertDialogDescription>
                  {plural(others.length, {
                    one: "# other browser will need to sign in again. This one stays signed in.",
                    other: "# other browsers will need to sign in again. This one stays signed in.",
                  })}
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>
                  <Trans>Cancel</Trans>
                </AlertDialogCancel>
                <AlertDialogAction variant="destructive" onClick={() => signOutOthers.mutate()}>
                  <Trans>Sign out others</Trans>
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
      <RowList aria-label={t`Sessions`}>
        {sessions.data?.map((session) => (
          <SessionRow key={session.id} session={session} />
        ))}
      </RowList>
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
      toast.success(t`Signed out ${device.label}`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Row
      lead={<Icon className="size-5 text-muted-foreground" />}
      title={
        <span className="flex flex-wrap items-center gap-2">
          {device.label}
          {session.current && (
            <span className="text-xs font-normal text-muted-foreground">
              <Trans>This browser</Trans>
            </span>
          )}
        </span>
      }
      meta={
        <Trans>
          {providerLabel(session.provider)} ·{" "}
          {session.current ? t`Active now` : t`Active ${formatRelative(session.last_used_at)}`} ·
          Signed in {formatDate(session.created_at)}
        </Trans>
      }
    >
      {!session.current && (
        <Button
          variant="outline"
          size="sm"
          onClick={() => signOut.mutate()}
          disabled={signOut.isPending}
        >
          <Trans>Sign out</Trans>
        </Button>
      )}
    </Row>
  );
}
