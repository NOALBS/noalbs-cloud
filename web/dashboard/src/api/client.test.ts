import { afterEach, describe, expect, it, vi } from "vitest";
import {
  ApiRequestError,
  fetchWithTimeout,
  NetworkError,
  REQUEST_TIMEOUT_MS,
  unwrap,
} from "./client";

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
    const result = unwrap(Promise.resolve({ error: "I'm a teapot", response: response(418) }));

    await expect(result).rejects.toMatchObject({ status: 418, code: "unknown" });
  });

  it("says NOALBS isn't answering for a proxy's 502 to 504", async () => {
    const result = unwrap(Promise.resolve({ error: "Bad Gateway", response: response(502) }));

    await expect(result).rejects.toMatchObject({ status: 502, code: "unavailable" });
  });
});

describe("fetchWithTimeout", () => {
  /** A `fetch` that never answers, but rejects when its request is aborted. */
  const hangingFetch = (request: Request) =>
    new Promise<Response>((_, reject) =>
      request.signal.addEventListener("abort", () => reject(request.signal.reason)),
    );

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("turns a network failure into a NetworkError", async () => {
    vi.stubGlobal("fetch", () => Promise.reject(new TypeError("Failed to fetch")));

    const result = fetchWithTimeout(new Request("http://localhost/api/me"));
    await expect(result).rejects.toBeInstanceOf(NetworkError);
    await expect(result).rejects.toMatchObject({ timedOut: false });
  });

  it("gives up on a request that never answers", async () => {
    // Fake timers don't reach `AbortSignal.timeout`, so fire it by hand.
    const timeout = new AbortController();
    const timeoutSpy = vi.spyOn(AbortSignal, "timeout").mockReturnValue(timeout.signal);
    vi.stubGlobal("fetch", hangingFetch);

    const result = fetchWithTimeout(new Request("http://localhost/api/me"));
    expect(timeoutSpy).toHaveBeenCalledWith(REQUEST_TIMEOUT_MS);
    timeout.abort(new DOMException("signal timed out", "TimeoutError"));
    await expect(result).rejects.toMatchObject({ timedOut: true });
  });

  it("passes the caller's own abort through", async () => {
    vi.stubGlobal("fetch", hangingFetch);
    const controller = new AbortController();

    const result = fetchWithTimeout(
      new Request("http://localhost/api/me", { signal: controller.signal }),
    );
    controller.abort();
    await expect(result).rejects.not.toBeInstanceOf(NetworkError);
  });
});
