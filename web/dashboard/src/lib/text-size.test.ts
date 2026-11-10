import { describe, expect, it } from "vitest";
import { parseTextSize } from "./text-size";

describe("text size", () => {
  it("reads supported sizes and defaults unknown values", () => {
    expect(parseTextSize("default")).toBe("default");
    expect(parseTextSize("large")).toBe("large");
    expect(parseTextSize("larger")).toBe("larger");
    expect(parseTextSize(null)).toBe("default");
    expect(parseTextSize("huge")).toBe("default");
  });
});
