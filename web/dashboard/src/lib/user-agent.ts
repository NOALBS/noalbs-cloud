/** A short, human description of a browser from its user agent, e.g. "Firefox on Linux". */
export function describeUserAgent(userAgent: string | null | undefined): {
  label: string;
  mobile: boolean;
} {
  if (!userAgent) return { label: "Unknown browser", mobile: false };
  const ua = userAgent;

  // Order matters: Edge and Opera also say Chrome; Chrome also says Safari.
  const browser = /Edg(e|A|iOS)?\//.test(ua)
    ? "Edge"
    : /OPR\/|Opera/.test(ua)
      ? "Opera"
      : /Firefox\/|FxiOS\//.test(ua)
        ? "Firefox"
        : /Chrome\/|CriOS\//.test(ua)
          ? "Chrome"
          : /Safari\//.test(ua)
            ? "Safari"
            : null;

  const os = /iPhone/.test(ua)
    ? "iPhone"
    : /iPad/.test(ua)
      ? "iPad"
      : /Android/.test(ua)
        ? "Android"
        : /Windows/.test(ua)
          ? "Windows"
          : /Mac OS X|Macintosh/.test(ua)
            ? "macOS"
            : /CrOS/.test(ua)
              ? "ChromeOS"
              : /Linux/.test(ua)
                ? "Linux"
                : null;

  const mobile = /Mobile|iPhone|Android/.test(ua);
  const label =
    browser && os ? `${browser} on ${os}` : (browser ?? os ?? ua.split(" ")[0] ?? "Unknown");
  return { label, mobile };
}
