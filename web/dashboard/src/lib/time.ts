import { currentLocale } from "@/lib/locale";

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 3600],
  ["month", 30 * 24 * 3600],
  ["week", 7 * 24 * 3600],
  ["day", 24 * 3600],
  ["hour", 3600],
  ["minute", 60],
];

/** "in 6 days", "tomorrow", "3 hours ago", "now". */
export function formatRelative(date: string | Date, now: Date = new Date()): string {
  const seconds = Math.round((new Date(date).getTime() - now.getTime()) / 1000);
  for (const [unit, size] of UNITS) {
    if (Math.abs(seconds) >= size) {
      return new Intl.RelativeTimeFormat(currentLocale(), { numeric: "auto" }).format(
        Math.round(seconds / size),
        unit,
      );
    }
  }
  return "now";
}

export function formatDate(date: string | Date): string {
  return new Date(date).toLocaleDateString(currentLocale(), {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}
