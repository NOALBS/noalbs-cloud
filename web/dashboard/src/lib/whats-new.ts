/** A "What's new" post as the dashboard gets it (see `src/whats-new/posts.ts`). */
export interface WhatsNewPost {
  /** The file name without `.md`: `2026-11-02-scene-picker`. IDs sort by date. */
  id: string;
  /** `YYYY-MM-DD`. */
  date: string;
  title: string;
  /** Plain text, for the popup. */
  summary: string;
  image: { url: string; width: number; height: number; alt: string } | null;
  /** Whether it pops up in the dashboard; off for small news, only on the page. */
  popup: boolean;
}

/** The public page with every post. */
export const WHATS_NEW_PATH = "/whats-new";

export const postUrl = (id: string) => `${WHATS_NEW_PATH}#${id}`;

/**
 * The posts to pop up, newest first: newer than the last one seen or, before the
 * first, than the account (posts from before someone signed up aren't news).
 */
export function unseenPosts(
  posts: WhatsNewPost[],
  seen: string | null | undefined,
  createdAt: string | null | undefined,
): WhatsNewPost[] {
  const signedUp = createdAt?.slice(0, 10);
  return posts.filter(
    (post) => post.popup && (seen ? post.id > seen : !signedUp || post.date > signedUp),
  );
}
