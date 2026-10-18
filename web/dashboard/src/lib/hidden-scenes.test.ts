import { describe, expect, it } from "vitest";
import { parseHidden, sceneKey } from "./hidden-scenes";

describe("hidden scenes", () => {
  it("keys scenes by UUID, falling back to the name", () => {
    expect(sceneKey({ uuid: "abc", name: "LIVE" })).toBe("abc");
    expect(sceneKey({ uuid: null, name: "LIVE" })).toBe("name:LIVE");
  });

  it("reads stored lists and ignores junk", () => {
    expect(parseHidden(null)).toEqual([]);
    expect(parseHidden('["a","b"]')).toEqual(["a", "b"]);
    expect(parseHidden('["a",3,null]')).toEqual(["a"]);
    expect(parseHidden("{oops")).toEqual([]);
    expect(parseHidden('{"a":1}')).toEqual([]);
  });
});
