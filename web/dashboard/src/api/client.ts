import { t } from "@lingui/core/macro";
import createClient from "openapi-fetch";
import type { components, paths } from "./schema";

export type ErrorBody = components["schemas"]["ErrorBody"];

/**
 * How long a request may take. The API gives up on OBS after 5 s and on Moblin
 * after 10 s, so this only fires when the network is the problem (a phone on a
 * bad signal), where waiting forever would leave buttons stuck.
 */
export const REQUEST_TIMEOUT_MS = 15_000;

/** A request that got no answer: the network failed, or it took too long. */
export class NetworkError extends Error {
  constructor(readonly timedOut: boolean) {
    super(
      timedOut
        ? t`NOALBS didn't answer in time. Check your connection and try again.`
        : t`Couldn't reach NOALBS. Check your connection and try again.`,
    );
    this.name = "NetworkError";
  }
}

/**
 * `fetch` that aborts after {@link REQUEST_TIMEOUT_MS}, keeping the caller's own
 * signal, and rejects with a {@link NetworkError} instead of the browser's own
 * ("Failed to fetch", "Load failed", …).
 */
export async function fetchWithTimeout(request: Request, init?: RequestInit): Promise<Response> {
  const timeout = AbortSignal.timeout(REQUEST_TIMEOUT_MS);
  // Older Safari has no `AbortSignal.any`; there only the timeout cancels.
  const signal = AbortSignal.any ? AbortSignal.any([request.signal, timeout]) : timeout;
  try {
    return await fetch(new Request(request, { signal }), init);
  } catch (error) {
    // The caller cancelled (TanStack Query does when a query is dropped): not a failure.
    if (request.signal.aborted) throw error;
    throw new NetworkError(timeout.aborted);
  }
}

/**
 * Typed client for the NOALBS API. Types are generated from the API's OpenAPI
 * document (`pnpm gen:api`), so a changed endpoint fails `pnpm typecheck`.
 */
export const api = createClient<paths>({
  baseUrl: "/api",
  credentials: "include",
  fetch: fetchWithTimeout,
});

/** An API call that failed; `code` is the API's stable error code. */
export class ApiRequestError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = "ApiRequestError";
    this.status = status;
    this.code = code;
  }
}

type FetchResult<T> = {
  data?: T;
  error?: unknown;
  response: Response;
};

function isErrorBody(value: unknown): value is ErrorBody {
  if (typeof value !== "object" || value === null || !("error" in value)) {
    return false;
  }
  const detail = (value as { error: unknown }).error;
  return typeof detail === "object" && detail !== null && "code" in detail && "message" in detail;
}

/**
 * Resolves an openapi-fetch result to its data, or throws an
 * {@link ApiRequestError}. Lets TanStack Query treat API errors as failures.
 */
export async function unwrap<T>(request: Promise<FetchResult<T>>): Promise<T> {
  const { data, error, response } = await request;

  if (error !== undefined || !response.ok) {
    if (isErrorBody(error)) {
      throw new ApiRequestError(response.status, error.error.code, error.error.message);
    }
    // A proxy's error page rather than the API's: NOALBS is down or restarting.
    if (response.status >= 502 && response.status <= 504) {
      throw new ApiRequestError(
        response.status,
        "unavailable",
        t`NOALBS isn't answering right now. Try again in a moment.`,
      );
    }
    throw new ApiRequestError(response.status, "unknown", t`Request failed (${response.status})`);
  }

  return data as T;
}
