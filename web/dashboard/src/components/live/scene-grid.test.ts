import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readSceneView } from "./scene-grid";

describe("scene view preference", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => vi.restoreAllMocks());
  it("defaults absent and invalid preferences to a grid", () => {
    expect(readSceneView("one")).toBe("grid");
    localStorage.setItem("noalbs-scene-view:one", "invalid");
    expect(readSceneView("one")).toBe("grid");
  });
  it("restores list mode for only its switcher", () => {
    localStorage.setItem("noalbs-scene-view:one", "list");
    expect(readSceneView("one")).toBe("list");
    expect(readSceneView("two")).toBe("grid");
  });
  it("remains usable when browser storage is denied", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    expect(readSceneView("one")).toBe("grid");
  });
});
