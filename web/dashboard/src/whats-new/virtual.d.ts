declare module "virtual:whats-new" {
  /** Every post, newest first, built from `whats-new/*.md` by `src/whats-new/plugin.ts`. */
  const posts: import("@/lib/whats-new").WhatsNewPost[];
  export default posts;
}
