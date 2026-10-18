import { describe, expect, it } from "vitest";
import { parseWorkspacesSearch } from "./my-workspaces";

describe("parseWorkspacesSearch", () => {
  it("keeps what fits", () => {
    expect(
      parseWorkspacesSearch({
        q: "kate",
        page: "3",
        role: "operator",
        state: "live",
        sort: "name",
      }),
    ).toEqual({ q: "kate", page: 3, role: "operator", state: "live", sort: "name" });
  });

  it("drops what doesn't", () => {
    expect(
      parseWorkspacesSearch({ q: "", page: "1", role: "boss", state: "asleep", sort: "newest" }),
    ).toEqual({
      q: undefined,
      page: undefined,
      role: undefined,
      state: undefined,
      sort: undefined,
    });
    expect(parseWorkspacesSearch({ page: "2.5" }).page).toBeUndefined();
    expect(parseWorkspacesSearch({ q: 5 }).q).toBeUndefined();
  });
});
