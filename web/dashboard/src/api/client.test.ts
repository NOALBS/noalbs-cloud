import { describe, expect, it } from "vitest";
import { ApiRequestError, unwrap } from "./client";

const response = (status: number) => new Response(null, { status });

describe("unwrap", () => {
  it("returns data for a successful response", async () => {
    await expect(
      unwrap(Promise.resolve({ data: { id: 1 }, response: response(200) })),
    ).resolves.toEqual({
      id: 1,
    });
  });

  it("throws the API's error code and message", async () => {
    const error = { error: { code: "slug_taken", message: "This URL is already taken" } };
    const result = unwrap(Promise.resolve({ error, response: response(409) }));

    await expect(result).rejects.toBeInstanceOf(ApiRequestError);
    await expect(result).rejects.toMatchObject({
      status: 409,
      code: "slug_taken",
      message: "This URL is already taken",
    });
  });

  it("falls back to a generic error for unexpected bodies", async () => {
    const result = unwrap(Promise.resolve({ error: "Bad Gateway", response: response(502) }));

    await expect(result).rejects.toMatchObject({ status: 502, code: "unknown" });
  });
});
