import { describe, expect, it } from "vitest";
import { unseenPosts, type WhatsNewPost } from "./whats-new";

const post = (id: string, popup = true): WhatsNewPost => ({
  id,
  date: id.slice(0, 10),
  title: id,
  summary: "",
  image: null,
  popup,
});

// Newest first, as the build lists them.
const posts = [
  post("2026-12-01-raids"),
  post("2026-11-20-typo-fix", false),
  post("2026-11-05-scene-picker"),
  post("2026-11-02-moblin"),
];

const ids = (list: WhatsNewPost[]) => list.map((item) => item.id);

describe("unseenPosts", () => {
  it("shows what came after the last post seen", () => {
    expect(ids(unseenPosts(posts, "2026-11-02-moblin", "2026-01-01T10:00:00Z"))).toEqual([
      "2026-12-01-raids",
      "2026-11-05-scene-picker",
    ]);
    expect(unseenPosts(posts, "2026-12-01-raids", "2026-01-01T10:00:00Z")).toEqual([]);
  });

  it("goes by post, not by when it was seen, so a post deployed late still shows", () => {
    // Seen on 2026-11-10, but the 2026-11-05 post only shipped afterwards.
    expect(ids(unseenPosts(posts, "2026-11-02-moblin", "2026-01-01T10:00:00Z"))).toContain(
      "2026-11-05-scene-picker",
    );
  });

  it("skips posts from before the account, and same-day ones", () => {
    expect(ids(unseenPosts(posts, null, "2026-11-05T09:00:00Z"))).toEqual(["2026-12-01-raids"]);
  });

  it("never pops up posts that are only for the page", () => {
    expect(ids(unseenPosts(posts, null, null))).not.toContain("2026-11-20-typo-fix");
  });
});
