import posts from "virtual:whats-new";
import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import { api } from "@/api/client";
import { meQuery } from "@/api/queries";
import type { Me } from "@/api/types";
import { currentLocale } from "@/lib/locale";
import { postUrl, unseenPosts, WHATS_NEW_PATH } from "@/lib/whats-new";

/** Whether there are posts the user hasn't seen, for the dot in the account menu. */
export function hasUnseenPosts(me: Me): boolean {
  return unseenPosts(posts, me.whats_new_seen, me.created_at).length > 0;
}

/**
 * The newest "What's new" post the user hasn't seen, in a corner: it doesn't take
 * focus or cover the page, as people use the dashboard while they're live.
 * Closing it or reading on marks every post up to now seen.
 */
export function WhatsNewCard({ me }: { me: Me }) {
  const queryClient = useQueryClient();
  const unseen = unseenPosts(posts, me.whats_new_seen, me.created_at);
  const post = unseen[0];
  if (!post) return null;

  const markSeen = () => {
    const newest = posts[0]?.id;
    if (!newest) return;
    queryClient.setQueryData(
      meQuery.queryKey,
      (current) => current && { ...current, whats_new_seen: newest },
    );
    // If it fails, the card just comes back next time.
    void api.PUT("/me/whats-new-seen", { body: { post_id: newest } });
  };

  const date = new Date(`${post.date}T00:00:00Z`).toLocaleDateString(currentLocale(), {
    dateStyle: "medium",
    timeZone: "UTC",
  });

  return (
    <section
      aria-label={t`What's new`}
      className="fixed inset-x-3 bottom-[calc(5.5rem+env(safe-area-inset-bottom))] z-30 overflow-hidden rounded-xl border border-divider bg-popover text-popover-foreground shadow-lg md:inset-x-auto md:right-6 md:bottom-6 md:w-88"
    >
      {post.image && (
        <img
          src={post.image.url}
          alt={post.image.alt}
          width={post.image.width}
          height={post.image.height}
          className="aspect-video w-full border-b border-divider object-cover"
        />
      )}
      <div className="flex flex-col gap-1.5 p-4">
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs text-muted-foreground">
            <Trans>What's new</Trans> · {date}
          </span>
          <button
            type="button"
            onClick={markSeen}
            aria-label={t`Dismiss`}
            className="-my-1.5 -mr-2 flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </div>
        <h2 className="text-[15px] leading-snug font-semibold">{post.title}</h2>
        <p className="text-sm text-muted-foreground">{post.summary}</p>
        <div className="mt-1.5 flex items-center gap-4 text-sm font-medium">
          <a
            href={postUrl(post.id)}
            target="_blank"
            rel="noopener"
            onClick={markSeen}
            className="text-primary hover:underline"
          >
            <Trans>Read more</Trans>
          </a>
          {unseen.length > 1 && (
            <a
              href={WHATS_NEW_PATH}
              target="_blank"
              rel="noopener"
              onClick={markSeen}
              className="text-muted-foreground hover:text-foreground"
            >
              <Trans>See all updates</Trans>
            </a>
          )}
        </div>
      </div>
    </section>
  );
}
