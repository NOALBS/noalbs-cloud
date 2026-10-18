/** The platforms people sign in with and link chat channels from, and how their buttons look. */
export const PLATFORMS = [
  {
    provider: "TWITCH",
    path: "twitch",
    label: "Twitch",
    button: "bg-[#9146FF] text-white hover:bg-[#9146FF]/90",
    /** A quiet tinted mark, for lists. */
    mark: "bg-[#9146FF]/15 text-[#6D28D9] dark:text-[#A970FF]",
  },
  {
    provider: "KICK",
    path: "kick",
    label: "Kick",
    button: "bg-[#53FC18] text-[#0B0D10] hover:bg-[#53FC18]/90",
    mark: "bg-[#53FC18]/25 text-[#1B6B00] dark:bg-[#53FC18]/15 dark:text-[#53FC18]",
  },
  {
    provider: "YOUTUBE",
    path: "youtube",
    label: "YouTube",
    button: "bg-[#FF0000] text-white hover:bg-[#FF0000]/90",
    mark: "bg-[#FF0000]/12 text-[#CC0000] dark:bg-[#FF0000]/20 dark:text-[#FF6B63]",
  },
] as const;

export type PlatformPath = (typeof PLATFORMS)[number]["path"];

export function platformLabel(path: string): string {
  return PLATFORMS.find((platform) => platform.path === path)?.label ?? path;
}
