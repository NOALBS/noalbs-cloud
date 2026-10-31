import { t } from "@lingui/core/macro";
import { ApiRequestError } from "@/api/client";
import { translateApiMessage } from "@/lib/api-messages";

/** A message to show the user for any thrown error. */
export function errorMessage(error: unknown): string {
  if (error instanceof ApiRequestError) return translateApiMessage(error.message);
  if (error instanceof Error && error.message) return error.message;
  return t`Something went wrong. Please try again.`;
}

export function isApiError(error: unknown, code: string): boolean {
  return error instanceof ApiRequestError && error.code === code;
}
