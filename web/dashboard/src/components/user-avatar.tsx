import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { cn } from "@/lib/utils";
import { initials } from "./workspace-tile";

export function UserAvatar({
  name,
  avatarUrl,
  className,
}: {
  name: string | null | undefined;
  avatarUrl?: string | null;
  className?: string;
}) {
  return (
    <Avatar className={cn("size-8", className)}>
      {avatarUrl && <AvatarImage src={avatarUrl} alt="" />}
      <AvatarFallback className="bg-low/15 text-xs font-semibold text-low">
        {initials(name || "?")}
      </AvatarFallback>
    </Avatar>
  );
}

const PROVIDER_LABELS: Record<string, string> = {
  TWITCH: "Twitch",
  KICK: "Kick",
  YOUTUBE: "YouTube",
  DISCORD: "Discord",
  DEV: "Dev login",
};

export function providerLabel(provider: string | null | undefined): string {
  return provider ? (PROVIDER_LABELS[provider] ?? provider) : "";
}
