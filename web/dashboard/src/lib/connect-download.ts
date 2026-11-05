export type ConnectPlatform = "windows" | "macos" | "linux";

type ConnectPlatformInput = {
  userAgent: string;
  platform?: string;
  maxTouchPoints?: number;
};

type ConnectDownload = { label: string; url: string };

const RELEASE_DOWNLOAD_URL = "https://github.com/NOALBS/noalbs-cloud/releases/latest/download/";

export const CONNECT_RELEASES_URL = "https://github.com/NOALBS/noalbs-cloud/releases/latest";

const downloads: Record<ConnectPlatform, { primary: ConnectDownload; others: ConnectDownload[] }> =
  {
    windows: {
      primary: {
        label: "Windows",
        url: `${RELEASE_DOWNLOAD_URL}NOALBS-Cloud-Connect-windows-x64-setup.exe`,
      },
      others: [],
    },
    macos: {
      primary: {
        label: "macOS",
        url: `${RELEASE_DOWNLOAD_URL}NOALBS-Cloud-Connect-macos-universal.dmg`,
      },
      others: [],
    },
    linux: {
      primary: {
        label: "Linux",
        url: `${RELEASE_DOWNLOAD_URL}NOALBS-Cloud-Connect-linux-x86_64.AppImage`,
      },
      others: [
        {
          label: ".deb",
          url: `${RELEASE_DOWNLOAD_URL}NOALBS-Cloud-Connect-linux-amd64.deb`,
        },
        {
          label: ".rpm",
          url: `${RELEASE_DOWNLOAD_URL}NOALBS-Cloud-Connect-linux-x86_64.rpm`,
        },
      ],
    },
  };

export function detectConnectPlatform({
  userAgent,
  platform,
  maxTouchPoints = 0,
}: ConnectPlatformInput): ConnectPlatform | null {
  const normalizedPlatform = platform?.trim().toLowerCase();
  const normalizedUserAgent = userAgent.toLowerCase();
  const hints = `${normalizedPlatform ?? ""} ${normalizedUserAgent}`;

  if (/android|iphone|ipad|ipod|ipad os|ios|cros|chrome\s?os/.test(hints)) return null;
  if (normalizedPlatform && /mac/.test(normalizedPlatform) && maxTouchPoints > 1) return null;

  if (normalizedPlatform) {
    if (/win/.test(normalizedPlatform)) return "windows";
    if (/mac/.test(normalizedPlatform)) return "macos";
    if (/linux/.test(normalizedPlatform)) return "linux";
    return null;
  }

  if (/windows/.test(normalizedUserAgent)) return "windows";
  if (/macintosh|mac os x/.test(normalizedUserAgent)) {
    return maxTouchPoints > 1 ? null : "macos";
  }
  if (/linux/.test(normalizedUserAgent)) return "linux";
  return null;
}

export function connectDownloads(platform: ConnectPlatform | null): {
  primary?: ConnectDownload;
  others: ConnectDownload[];
} {
  if (!platform) return { others: [] };
  const { primary, others } = downloads[platform];
  return { primary: { ...primary }, others: others.map((download) => ({ ...download })) };
}
