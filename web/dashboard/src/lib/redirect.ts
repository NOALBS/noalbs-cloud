/**
 * First path segments served by something other than the dashboard app (the
 * API, obs-link, the browser source, webhooks, files, static pages) or that
 * would loop (`login`). Never a place to go back to after signing in: sending a
 * signed-in person to `/api/auth/twitch` would start another sign-in.
 */
const NOT_THE_APP = [
  "api",
  "link",
  "source",
  "chat",
  "assets",
  "privacy",
  "terms",
  "licenses",
  "login",
];

/** Where to go after signing in: a dashboard path on this site, else `/`. */
export function safeRedirect(target: string | undefined): string {
  if (!target?.startsWith("/") || target.startsWith("//") || target.startsWith("/\\")) return "/";
  const first = (target.slice(1).split(/[/?#]/, 1)[0] ?? "").toLowerCase();
  return NOT_THE_APP.includes(first) || first.endsWith(".html") ? "/" : target;
}
