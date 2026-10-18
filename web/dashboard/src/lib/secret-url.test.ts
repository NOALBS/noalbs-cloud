import { describe, expect, it } from "vitest";
import { maskUrl } from "./secret-url";

describe("maskUrl", () => {
  it("keeps only the scheme", () => {
    expect(maskUrl("https://cloud.belabox.net/stats/abc123SECRET")).toBe("https://••••••••");
    expect(maskUrl("http://admin:hunter2@203.0.113.7:9997/v3/paths/get/live?key=x")).toBe(
      "http://••••••••",
    );
    expect(maskUrl("http://localhost:5173/source/#token")).toBe("http://••••••••");
  });

  it("masks everything it can't parse", () => {
    expect(maskUrl("cloud.belabox.net/stats/abc")).toBe("••••••••");
  });
});
