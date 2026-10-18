import createClient from "openapi-fetch";
import type { components, paths } from "./schema";

export type ErrorBody = components["schemas"]["ErrorBody"];

/**
 * Typed client for the NOALBS API. Types are generated from the API's OpenAPI
 * document (`pnpm gen:api`), so a changed endpoint fails `pnpm typecheck`.
 */
export const api = createClient<paths>({
  baseUrl: "/api",
  credentials: "include",
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
    throw new ApiRequestError(response.status, "unknown", `Request failed (${response.status})`);
  }

  return data as T;
}
