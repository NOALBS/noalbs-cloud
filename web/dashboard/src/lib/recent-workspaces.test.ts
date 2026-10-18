import { beforeEach, describe, expect, it } from "vitest";
import {
  MENU_WORKSPACES,
  menuWorkspaces,
  recentWorkspaceIds,
  rememberWorkspace,
} from "./recent-workspaces";

const many = (count: number) => Array.from({ length: count }, (_, i) => ({ id: `w${i}` }));
const ids = (list: { id: string }[]) => list.map((workspace) => workspace.id);

describe("menuWorkspaces", () => {
  it("lists everything when there are few", () => {
    const all = many(MENU_WORKSPACES);
    expect(menuWorkspaces(all, "w2", ["w5"])).toEqual({ shown: all, hidden: 0 });
  });

  it("lists the current one first, then the recent ones, when there are many", () => {
    const { shown, hidden } = menuWorkspaces(many(100), "w40", ["w7", "w40", "w3", "gone"]);
    expect(ids(shown)).toEqual(["w40", "w7", "w3"]);
    expect(hidden).toBe(97);
  });

  it("never lists more than the most", () => {
    const recent = many(50).map((workspace) => workspace.id);
    const { shown } = menuWorkspaces(many(100), "w99", recent);
    expect(shown).toHaveLength(MENU_WORKSPACES);
    expect(shown[0]?.id).toBe("w99");
  });

  it("is not empty for someone with no history", () => {
    const { shown, hidden } = menuWorkspaces(many(100), null, []);
    expect(ids(shown)).toEqual(["w0", "w1", "w2"]);
    expect(hidden).toBe(97);
  });
});

describe("recent workspaces", () => {
  beforeEach(() => localStorage.clear());

  it("remembers the newest first, without repeats", () => {
    rememberWorkspace("a");
    rememberWorkspace("b");
    rememberWorkspace("a");
    expect(recentWorkspaceIds()).toEqual(["a", "b"]);
  });

  it("keeps only the latest thirty", () => {
    for (let i = 0; i < 40; i++) rememberWorkspace(`w${i}`);
    const recent = recentWorkspaceIds();
    expect(recent).toHaveLength(30);
    expect(recent[0]).toBe("w39");
  });

  it("copes with junk in storage", () => {
    localStorage.setItem("noalbs-recent-workspaces", "not json");
    expect(recentWorkspaceIds()).toEqual([]);
    localStorage.setItem("noalbs-recent-workspaces", JSON.stringify([1, "a", null]));
    expect(recentWorkspaceIds()).toEqual(["a"]);
  });
});
