import { describe, expect, it } from "vitest";
import { safeRedirect } from "./redirect";

describe("safeRedirect", () => {
  it("keeps dashboard paths", () => {
    expect(safeRedirect("/nomad-live/s/abc/live?x=1")).toBe("/nomad-live/s/abc/live?x=1");
    expect(safeRedirect("/admin/users")).toBe("/admin/users");
    expect(safeRedirect("/apiary")).toBe("/apiary");
  });

  it("never sends you back into a sign-in or off the app", () => {
    expect(safeRedirect("/api/auth/twitch?redirect=%2F")).toBe("/");
    expect(safeRedirect("/API/auth/kick")).toBe("/");
    expect(safeRedirect("/login?redirect=/x")).toBe("/");
    expect(safeRedirect("/source/#token")).toBe("/");
    expect(safeRedirect("/privacy")).toBe("/");
    expect(safeRedirect("/terms")).toBe("/");
    expect(safeRedirect("/licenses")).toBe("/");
    expect(safeRedirect("/landing.html")).toBe("/");
  });

  it("stays on this site", () => {
    expect(safeRedirect(undefined)).toBe("/");
    expect(safeRedirect("https://evil.example")).toBe("/");
    expect(safeRedirect("//evil.example")).toBe("/");
    expect(safeRedirect("/\\evil.example")).toBe("/");
  });
});
