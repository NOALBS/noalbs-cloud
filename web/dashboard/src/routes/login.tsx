import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, redirect, useNavigate } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";
import { type FormEvent, useState } from "react";
import { api, unwrap } from "@/api/client";
import { meQuery, providersQuery } from "@/api/queries";
import { LoginBackdrop } from "@/components/login-backdrop";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Wordmark } from "@/components/wordmark";
import { errorMessage } from "@/lib/errors";
import { PLATFORMS } from "@/lib/platforms";
import { safeRedirect } from "@/lib/redirect";
import { cn } from "@/lib/utils";

/**
 * The landing page. Caddy serves it at `/` to visitors without a session, which is
 * everyone who sees this page; in dev it's a file of its own.
 */
const SITE = import.meta.env.DEV ? "/landing.html" : "/";

interface LoginSearch {
  redirect?: string;
  /** Set by the API when sign-in was refused, e.g. `account_disabled`. */
  error?: string;
}

export const Route = createFileRoute("/login")({
  validateSearch: (search: Record<string, unknown>): LoginSearch => ({
    redirect: typeof search.redirect === "string" ? search.redirect : undefined,
    error: typeof search.error === "string" ? search.error : undefined,
  }),
  beforeLoad: async ({ context, search }) => {
    const me = await context.queryClient.fetchQuery(meQuery).catch(() => null);
    if (me) throw redirect({ href: safeRedirect(search.redirect) });
  },
  component: LoginPage,
});

function LoginPage() {
  const { redirect: target, error } = Route.useSearch();
  const next = safeRedirect(target);

  return (
    <div className="isolate flex min-h-dvh flex-col">
      <LoginBackdrop />
      {/* Like the landing page's header, so the two read as one site. */}
      <header className="mx-auto flex h-16 w-full max-w-6xl items-center justify-between gap-4 px-5 sm:px-8">
        <a href={SITE} aria-label="NOALBS Cloud">
          <Wordmark />
        </a>
        <a
          href={SITE}
          className="-mr-3 flex items-center gap-1.5 rounded-lg px-3 py-2 text-sm font-medium text-muted-foreground hover:text-foreground"
        >
          <ArrowLeft className="size-4" />
          Back to the site
        </a>
      </header>

      <main className="flex flex-1 items-center justify-center px-5 pt-4 pb-12 sm:px-8 sm:py-12">
        <section className="flex w-full max-w-md flex-col gap-6 rounded-2xl border bg-card p-6 sm:p-8">
          <SignIn next={next} error={error} />
        </section>
      </main>

      <footer className="mx-auto flex w-full max-w-6xl justify-end px-5 pb-6 text-xs text-muted-foreground sm:px-8">
        <span className="flex gap-4">
          <a href="/privacy" className="hover:text-foreground">
            Privacy
          </a>
          <a href="/terms" className="hover:text-foreground">
            Terms
          </a>
          <a href="https://discord.gg/efWu5HWM2u" className="hover:text-foreground">
            Discord
          </a>
        </span>
      </footer>
    </div>
  );
}

/** The sign-in card's content: providers, refusals, dev login and the terms notice. */
function SignIn({ next, error }: { next: string; error: string | undefined }) {
  const providers = useQuery(providersQuery);

  return (
    <>
      <h1 className="text-2xl font-bold">Sign in to NOALBS Cloud</h1>

      {error === "account_disabled" && (
        <p className="rounded-xl border border-destructive/40 bg-destructive/10 p-4 text-sm">
          This account has been disabled. If you think that’s a mistake, email{" "}
          <a
            href="mailto:support@noalbs.com"
            className="font-semibold text-primary hover:underline"
          >
            support@noalbs.com
          </a>
          .
        </p>
      )}
      {error === "no_youtube_channel" && (
        <p className="rounded-xl border border-destructive/40 bg-destructive/10 p-4 text-sm">
          That Google account has no YouTube channel. Sign in again and pick the account (or brand
          account) you stream with.
        </p>
      )}

      <div className="flex flex-col gap-3">
        {PLATFORMS.filter((platform) => providers.data?.[platform.path]).map((platform) => (
          <a
            key={platform.path}
            href={`/api/auth/${platform.path}?redirect=${encodeURIComponent(next)}`}
            className={cn(
              "flex h-13 items-center justify-center rounded-xl font-semibold transition-colors",
              platform.button,
            )}
          >
            Continue with {platform.label}
          </a>
        ))}
        {providers.data && !PLATFORMS.some((platform) => providers.data[platform.path]) && (
          <p className="rounded-xl border border-dashed p-4 text-sm text-muted-foreground">
            No sign-in providers are configured on this server.
          </p>
        )}
      </div>

      {providers.data?.dev_login && <DevLogin next={next} />}

      <p className="text-sm text-muted-foreground">
        By continuing you agree to the{" "}
        <a href="/terms" className="underline underline-offset-4 hover:text-foreground">
          terms of service
        </a>{" "}
        and the{" "}
        <a href="/privacy" className="underline underline-offset-4 hover:text-foreground">
          privacy policy
        </a>
        .
      </p>
    </>
  );
}

/** Local-development sign-in, shown only when the API enables it. */
function DevLogin({ next }: { next: string }) {
  const [username, setUsername] = useState("");
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const login = useMutation({
    mutationFn: () => unwrap(api.POST("/auth/dev-login", { body: { username } })),
    onSuccess: async () => {
      await queryClient.invalidateQueries();
      await navigate({ href: next });
    },
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    login.mutate();
  };

  return (
    <form
      onSubmit={submit}
      className="flex flex-col gap-3 rounded-xl border border-dashed border-low/50 p-4"
    >
      <Label htmlFor="dev-username">Dev login</Label>
      <div className="flex gap-2">
        <Input
          id="dev-username"
          value={username}
          onChange={(event) => setUsername(event.target.value)}
          placeholder="Any username"
          autoComplete="off"
        />
        <Button type="submit" disabled={!username.trim() || login.isPending}>
          Sign in
        </Button>
      </div>
      {login.error && <p className="text-sm text-destructive">{errorMessage(login.error)}</p>}
      <p className="text-xs text-muted-foreground">
        Enabled with <code className="font-mono">dev_login = true</code>. Never in production.
      </p>
    </form>
  );
}
