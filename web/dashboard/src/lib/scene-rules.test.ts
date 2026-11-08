import { describe, expect, it } from "vitest";
import {
  disabledServers,
  findDuplicate,
  ruleLabel,
  rulesWithServer,
  shadowedBy,
} from "./scene-rules";

const servers = [
  { id: "s1", name: "BELABOX", enabled: true },
  { id: "s2", name: "Home", enabled: false },
  { id: "s3", name: "Backpack", enabled: true },
];

const rule = (id: string, server_ids: string[], enabled = true) => ({ id, server_ids, enabled });

describe("scene rules", () => {
  it("names a rule by its servers in priority order", () => {
    expect(ruleLabel(rule("a", ["s3", "s1"]), servers)).toBe("BELABOX + Backpack");
    expect(ruleLabel(rule("a", ["s1", "gone"]), servers)).toBe("BELABOX");
  });

  it("finds an enabled rule above that always matches first", () => {
    const pair = rule("pair", ["s1", "s3"]);
    const all = rule("all", ["s1", "s2", "s3"]);
    expect(shadowedBy([pair, all], 1)).toBe(pair);
    expect(shadowedBy([all, pair], 1)).toBeUndefined();
    expect(shadowedBy([pair, all], 0)).toBeUndefined();
    expect(shadowedBy([rule("off", ["s1", "s3"], false), all], 1)).toBeUndefined();
    expect(shadowedBy([rule("other", ["s1", "s2"]), rule("x", ["s1", "s3"])], 1)).toBeUndefined();
  });

  it("finds the same servers in any order, except the rule being edited", () => {
    const rules = [rule("a", ["s1", "s3"]), rule("b", ["s1", "s2", "s3"])];
    expect(findDuplicate(rules, ["s3", "s1"])?.id).toBe("a");
    expect(findDuplicate(rules, ["s3", "s1"], "a")).toBeUndefined();
    expect(findDuplicate(rules, ["s1", "s2"])).toBeUndefined();
  });

  it("lists turned-off servers and the rules a server is in", () => {
    expect(disabledServers(rule("a", ["s1", "s2"]), servers).map((s) => s.id)).toEqual(["s2"]);
    const rules = [rule("a", ["s1", "s3"]), rule("b", ["s1", "s2"])];
    expect(rulesWithServer(rules, "s3").map((r) => r.id)).toEqual(["a"]);
    expect(rulesWithServer(rules, "s1")).toHaveLength(2);
  });
});
