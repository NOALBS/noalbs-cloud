import { describe, expect, it } from "vitest";
import { inviteTokenFrom, slugify } from "./slug";

describe("slugify", () => {
  it("makes URL-safe slugs", () => {
    expect(slugify("Nomad Live")).toBe("nomad-live");
    expect(slugify("  Weekend  Rides!! ")).toBe("weekend-rides");
    expect(slugify("Café Crème")).toBe("cafe-creme");
    expect(slugify("--Team__42--")).toBe("team-42");
  });

  it("stays within the length limit without a trailing dash", () => {
    const slug = slugify(`${"a".repeat(49)} b`);
    expect(slug.length).toBeLessThanOrEqual(50);
    expect(slug.endsWith("-")).toBe(false);
  });
});

describe("inviteTokenFrom", () => {
  const token = "k7p2qxAbCdEfGhIjKlMnOpQrStUvWxYz0123456789_";

  it("reads tokens from links and bare tokens", () => {
    expect(inviteTokenFrom(`https://noalbs.com/invite/${token}`)).toBe(token);
    expect(inviteTokenFrom(`noalbs.com/invite/${token}?x=1`)).toBe(token);
    expect(inviteTokenFrom(` ${token} `)).toBe(token);
  });

  it("rejects things that aren't invites", () => {
    expect(inviteTokenFrom("")).toBeNull();
    expect(inviteTokenFrom("hello")).toBeNull();
  });
});
