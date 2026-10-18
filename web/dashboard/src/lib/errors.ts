import { ApiRequestError } from "@/api/client";

/** A message to show the user for any thrown error. */
export function errorMessage(error: unknown): string {
  if (error instanceof ApiRequestError) return error.message;
  if (error instanceof Error && error.message) return error.message;
  return "Something went wrong. Please try again.";
}

export function isApiError(error: unknown, code: string): boolean {
  return error instanceof ApiRequestError && error.code === code;
}
