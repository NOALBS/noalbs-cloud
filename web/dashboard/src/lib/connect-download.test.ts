import { describe, expect, it } from "vitest";
import { CONNECT_RELEASES_URL, connectDownloads, detectConnectPlatform } from "./connect-download";

const platformCases: Array<{
  name: string;
  userAgent: string;
  platform?: string;
  maxTouchPoints?: number;
  expected: ReturnType<typeof detectConnectPlatform>;
}> = [
  {
    name: "Windows Chrome",
    userAgent:
      "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/140.0.0.0 Safari/537.36",
    platform: "Win32",
    expected: "windows",
  },
  {
    name: "Windows Edge",
    userAgent:
      "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0",
    expected: "windows",
  },
  {
    name: "Windows user agent mentioning Microsoft",
    userAgent:
      "Mozilla/5.0 (Windows NT 10.0; Win64; x64; Microsoft Outlook 16.0) AppleWebKit/537.36 Chrome/140.0.0.0 Safari/537.36",
    expected: "windows",
  },
  {
    name: "ARM64 Linux",
    userAgent: "Mozilla/5.0 (X11; Linux aarch64; rv:140.0) Gecko/20100101 Firefox/140.0",
    platform: "Linux aarch64",
    expected: null,
  },
  {
    name: "32-bit ARM Linux",
    userAgent: "Mozilla/5.0 (X11; Linux armv7l) AppleWebKit/537.36 Chrome/140.0.0.0 Safari/537.36",
    expected: null,
  },
  {
    name: "macOS Safari",
    userAgent:
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 Version/17.0 Safari/605.1.15",
    platform: "MacIntel",
    expected: "macos",
  },
  {
    name: "macOS Chrome",
    userAgent:
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 Chrome/140.0.0.0 Safari/537.36",
    expected: "macos",
  },
  {
    name: "iPadOS with a Mac user agent",
    userAgent:
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 Version/17.0 Safari/605.1.15",
    platform: "MacIntel",
    maxTouchPoints: 5,
    expected: null,
  },
  {
    name: "iPhone",
    userAgent:
      "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 Version/17.0 Mobile/15E148 Safari/604.1",
    platform: "iPhone",
    expected: null,
  },
  {
    name: "Android",
    userAgent:
      "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 Chrome/140.0.0.0 Mobile Safari/537.36",
    platform: "Linux armv8l",
    expected: null,
  },
  {
    name: "Ubuntu Firefox",
    userAgent: "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0",
    platform: "Linux x86_64",
    expected: "linux",
  },
  {
    name: "ChromeOS",
    userAgent:
      "Mozilla/5.0 (X11; CrOS x86_64 16093.62.0) AppleWebKit/537.36 Chrome/140.0.0.0 Safari/537.36",
    platform: "Chrome OS",
    expected: null,
  },
  {
    name: "unknown platform",
    userAgent: "Unknown runtime",
    platform: "Unknown",
    expected: null,
  },
] as const;

describe("detectConnectPlatform", () => {
  for (const { name, userAgent, platform, maxTouchPoints, expected } of platformCases) {
    it(name, () => {
      expect(
        detectConnectPlatform({
          userAgent,
          platform,
          maxTouchPoints,
        }),
      ).toBe(expected);
    });
  }

  it("prefers the supplied userAgentData platform over the user agent", () => {
    expect(
      detectConnectPlatform({
        userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
        platform: "macOS",
      }),
    ).toBe("macos");
  });
});

describe("connectDownloads", () => {
  it("publishes the latest releases page", () => {
    expect(CONNECT_RELEASES_URL).toBe("https://github.com/NOALBS/noalbs-cloud/releases/latest");
  });

  it("returns the Windows installer", () => {
    expect(connectDownloads("windows")).toEqual({
      primary: {
        label: "Windows",
        url: "https://github.com/NOALBS/noalbs-cloud/releases/latest/download/NOALBS-Cloud-Connect-windows-x64-setup.exe",
      },
      others: [],
    });
  });

  it("returns the universal macOS disk image", () => {
    expect(connectDownloads("macos")).toEqual({
      primary: {
        label: "macOS",
        url: "https://github.com/NOALBS/noalbs-cloud/releases/latest/download/NOALBS-Cloud-Connect-macos-universal.dmg",
      },
      others: [],
    });
  });

  it("returns the Linux AppImage and alternate packages", () => {
    expect(connectDownloads("linux")).toEqual({
      primary: {
        label: "Linux",
        url: "https://github.com/NOALBS/noalbs-cloud/releases/latest/download/NOALBS-Cloud-Connect-linux-x86_64.AppImage",
      },
      others: [
        {
          label: ".deb",
          url: "https://github.com/NOALBS/noalbs-cloud/releases/latest/download/NOALBS-Cloud-Connect-linux-amd64.deb",
        },
        {
          label: ".rpm",
          url: "https://github.com/NOALBS/noalbs-cloud/releases/latest/download/NOALBS-Cloud-Connect-linux-x86_64.rpm",
        },
      ],
    });
  });

  it("returns no platform-specific downloads when the platform is unknown", () => {
    expect(connectDownloads(null)).toEqual({ others: [] });
  });
});
