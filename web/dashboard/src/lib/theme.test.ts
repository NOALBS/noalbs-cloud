import { describe, expect, it } from "vitest";
import { parsePreference, resolveTheme } from "./theme";

describe("theme", () => {
  it("reads a saved choice, and anything else as Auto", () => {
    expect(parsePreference("light")).toBe("light");
    expect(parsePreference("dark")).toBe("dark");
    expect(parsePreference("system")).toBe("system");
    expect(parsePreference(null)).toBe("system");
    expect(parsePreference("purple")).toBe("system");
  });

  it("Auto follows the browser; a choice overrides it", () => {
    expect(resolveTheme("system", true)).toBe("light");
    expect(resolveTheme("system", false)).toBe("dark");
    expect(resolveTheme("dark", true)).toBe("dark");
    expect(resolveTheme("light", false)).toBe("light");
  });
});
