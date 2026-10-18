import type { Notice } from "@/api/types";

/** Where the notices this browser hid are kept. */
export const DISMISSED_KEY = "noalbs-dismissed-notices";

/** Reads the stored IDs, tolerating missing or malformed values. */
export function parseDismissed(raw: string | null): string[] {
  if (!raw) return [];
  try {
    const value: unknown = JSON.parse(raw);
    return Array.isArray(value) ? value.filter((item) => typeof item === "string") : [];
  } catch {
    return [];
  }
}

/** The notices to show: a critical one can't be hidden, whatever is stored. */
export function visibleNotices(notices: Notice[], dismissed: string[]): Notice[] {
  return notices.filter((notice) => !notice.dismissible || !dismissed.includes(notice.id));
}

/** Keeps only the dismissed IDs of notices still in force, so the list can't grow forever. */
export function pruneDismissed(dismissed: string[], notices: Notice[]): string[] {
  return dismissed.filter((id) => notices.some((notice) => notice.id === id));
}

/** A date for `<input type="datetime-local">` (local time, to the minute); empty for none. */
export function toLocalInput(iso: string | null): string {
  if (!iso) return "";
  const date = new Date(iso);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** What `toLocalInput` made, back to an ISO time; `null` when empty or not a date. */
export function fromLocalInput(value: string): string | null {
  if (!value) return null;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date.toISOString();
}

export const SEVERITIES = [
  { value: "info", label: "Info" },
  { value: "warning", label: "Warning" },
  { value: "critical", label: "Critical" },
] as const;
