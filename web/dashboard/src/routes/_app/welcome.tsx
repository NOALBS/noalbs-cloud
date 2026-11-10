import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient, useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { ArrowLeft, Check, Loader2, X } from "lucide-react";
import { type FormEvent, useState } from "react";
import { api, unwrap } from "@/api/client";
import { meQuery, queryKeys, slugAvailableQuery, workspacesQuery } from "@/api/queries";
import { MinimalHeader } from "@/components/minimal-header";
import { NoticeBanners } from "@/components/notice-banner";
import { SettingsRow } from "@/components/settings-row";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useDebouncedValue } from "@/hooks/use-debounced-value";
import { translateApiMessage } from "@/lib/api-messages";
import { errorMessage } from "@/lib/errors";
import { inviteTokenFrom, SLUG_MIN_LENGTH, slugify } from "@/lib/slug";
import { cn } from "@/lib/utils";

interface WelcomeSearch {
  mode?: "create" | "join";
}

export const Route = createFileRoute("/_app/welcome")({
  validateSearch: (search: Record<string, unknown>): WelcomeSearch => ({
    mode: search.mode === "create" || search.mode === "join" ? search.mode : undefined,
  }),
  loader: ({ context }) => context.queryClient.ensureQueryData(workspacesQuery),
  component: WelcomePage,
});

function WelcomePage() {
  const { mode } = Route.useSearch();
  const { data: me } = useSuspenseQuery(meQuery);
  const { data: workspaces } = useSuspenseQuery(workspacesQuery);
  const username = me.connections[0]?.username;
  const firstRun = workspaces.length === 0;

  return (
    <div className="flex min-h-dvh flex-col">
      <MinimalHeader me={me} />
      <NoticeBanners />
      <main className="flex flex-1 flex-col items-center gap-10 px-4 pt-12 pb-16 md:pt-20">
        <div className="flex max-w-xl flex-col items-center gap-2.5 text-center">
          <h1 className="font-display text-3xl font-extrabold tracking-tight md:text-4xl">
            {firstRun ? (username ? t`Welcome, ${username}` : t`Welcome`) : t`Add a workspace`}
          </h1>
          <p className="text-muted-foreground">
            <Trans>
              Create a workspace for your stream, or join the one your team already uses.
            </Trans>
          </p>
        </div>

        <section className="w-full max-w-2xl overflow-hidden rounded-2xl border bg-card shadow-tile">
          <CreateWorkspace autoFocus={mode !== "join"} />
          <JoinWorkspace autoFocus={mode === "join"} />
        </section>

        {!firstRun && workspaces[0] && (
          <Link
            to="/$workspace"
            params={{ workspace: me.last_workspace_slug ?? workspaces[0].url_slug }}
            className="flex items-center gap-2 text-sm font-medium text-primary hover:underline"
          >
            <ArrowLeft className="size-4" />
            <Trans>Back to my workspace</Trans>
          </Link>
        )}
      </main>
    </div>
  );
}

function CreateWorkspace({ autoFocus }: { autoFocus: boolean }) {
  const [name, setName] = useState("");
  const [customSlug, setCustomSlug] = useState<string | null>(null);
  const slug = customSlug ?? slugify(name);
  const debouncedSlug = useDebouncedValue(slug);
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const availability = useQuery({
    ...slugAvailableQuery(debouncedSlug),
    enabled: debouncedSlug.length >= SLUG_MIN_LENGTH,
  });
  const checking = slug !== debouncedSlug || availability.isFetching;
  const unavailable = !checking && availability.data?.available === false;

  const create = useMutation({
    mutationFn: () => unwrap(api.POST("/workspaces", { body: { name, url_slug: slug } })),
    onSuccess: async (workspace) => {
      await queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
      await queryClient.invalidateQueries({ queryKey: queryKeys.me });
      await navigate({ to: "/$workspace", params: { workspace: workspace.url_slug } });
    },
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    create.mutate();
  };

  return (
    <form onSubmit={submit} className="flex flex-col">
      <div className="flex flex-col gap-1.5 px-5 py-4">
        <h2 className="font-display text-xl font-bold">
          <Trans>Create a workspace</Trans>
        </h2>
        <p className="text-sm leading-relaxed text-muted-foreground">
          <Trans>You’ll be the owner. Add a switcher and invite your mods next.</Trans>
        </p>
      </div>

      <SettingsRow
        label={
          <Label htmlFor="workspace-name">
            <Trans>Workspace name</Trans>
          </Label>
        }
      >
        <Input
          id="workspace-name"
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder={t`Nomad Live`}
          maxLength={60}
          autoFocus={autoFocus}
          className="h-11"
        />
      </SettingsRow>

      <SettingsRow
        label={
          <Label htmlFor="workspace-slug">
            <Trans>URL</Trans>
          </Label>
        }
      >
        <div className="flex h-11 w-full items-center rounded-md border border-input bg-transparent pl-3 focus-within:ring-[3px] focus-within:ring-ring/50">
          <span className="max-w-1/2 truncate font-mono text-sm text-muted-foreground">
            {window.location.host}/
          </span>
          <input
            id="workspace-slug"
            value={slug}
            onChange={(event) => setCustomSlug(slugify(event.target.value) || event.target.value)}
            className="h-full min-w-0 flex-1 bg-transparent pr-3 font-mono text-sm outline-none"
            spellCheck={false}
            aria-describedby="slug-status"
          />
        </div>
        <p
          id="slug-status"
          className={cn(
            "flex min-h-5 items-center gap-1.5 text-xs",
            unavailable ? "text-destructive" : "text-muted-foreground",
          )}
        >
          {slug.length < SLUG_MIN_LENGTH ? (
            t`At least 3 characters: letters, numbers and dashes.`
          ) : checking ? (
            <>
              <Loader2 className="size-3.5 animate-spin" /> <Trans>Checking…</Trans>
            </>
          ) : unavailable ? (
            <>
              <X className="size-3.5" /> {translateApiMessage(availability.data?.message ?? "")}
            </>
          ) : availability.data?.available ? (
            <>
              <Check className="size-3.5 text-live" /> <Trans>Available</Trans>
            </>
          ) : null}
        </p>
      </SettingsRow>

      {create.error && (
        <p className="px-5 py-3 text-sm text-destructive">{errorMessage(create.error)}</p>
      )}

      <div className="border-t border-divider px-5 py-3">
        <Button
          type="submit"
          size="lg"
          className="h-11 w-full sm:w-auto"
          disabled={
            !name.trim() || slug.length < SLUG_MIN_LENGTH || unavailable || create.isPending
          }
        >
          {create.isPending ? t`Creating…` : t`Create workspace`}
        </Button>
      </div>
    </form>
  );
}

function JoinWorkspace({ autoFocus }: { autoFocus: boolean }) {
  const [link, setLink] = useState("");
  const [invalid, setInvalid] = useState(false);
  const navigate = useNavigate();

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const token = inviteTokenFrom(link);
    if (!token) {
      setInvalid(true);
      return;
    }
    void navigate({ to: "/invite/$token", params: { token } });
  };

  return (
    <form onSubmit={submit} className="flex flex-col border-t border-divider">
      <div className="flex flex-col gap-1.5 px-5 py-4">
        <h2 className="font-display text-xl font-bold">
          <Trans>Join with an invite</Trans>
        </h2>
        <p className="text-sm leading-relaxed text-muted-foreground">
          <Trans>Mod for someone? Paste the link they sent you.</Trans>
        </p>
      </div>

      <SettingsRow
        label={
          <Label htmlFor="invite-link">
            <Trans>Invite link</Trans>
          </Label>
        }
      >
        <Input
          id="invite-link"
          value={link}
          onChange={(event) => {
            setLink(event.target.value);
            setInvalid(false);
          }}
          placeholder={`${window.location.host}/invite/…`}
          autoFocus={autoFocus}
          aria-invalid={invalid}
          className="h-11 font-mono"
        />
        <p className={cn("text-xs", invalid ? "text-destructive" : "text-muted-foreground")}>
          {invalid
            ? t`That doesn’t look like an invite link.`
            : t`Invites expire, so ask for a new one if it doesn’t work.`}
        </p>
      </SettingsRow>

      <div className="border-t border-divider px-5 py-3">
        <Button
          type="submit"
          size="lg"
          variant="outline"
          className="h-11 w-full sm:w-auto"
          disabled={!link.trim()}
        >
          <Trans>Continue</Trans>
        </Button>
      </div>
    </form>
  );
}
