import { Link } from "@tanstack/react-router";
import type { Me } from "@/api/types";
import { providerLabel, UserAvatar } from "./user-avatar";
import { Wordmark } from "./wordmark";

/** Header for pages outside a workspace (first-run, invites). */
export function MinimalHeader({ me }: { me: Me }) {
  const connection = me.connections[0];

  return (
    <header className="flex h-16 items-center justify-between border-b border-divider px-4 md:px-8">
      <Link to="/" aria-label="Home">
        <Wordmark className="text-base" />
      </Link>
      <span className="flex items-center gap-2.5 text-sm text-muted-foreground">
        <span className="hidden sm:inline">
          {connection?.username}
          {connection && ` · ${providerLabel(connection.provider)}`}
        </span>
        <UserAvatar name={connection?.username} avatarUrl={connection?.avatar_url} />
      </span>
    </header>
  );
}
