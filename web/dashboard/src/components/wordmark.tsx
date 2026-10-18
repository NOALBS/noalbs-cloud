import { LogoMark } from "@/components/logo";
import { cn } from "@/lib/utils";

export function Wordmark({ className }: { className?: string }) {
  return (
    <span
      className={cn("flex items-center gap-2 font-mono text-lg font-bold tracking-wide", className)}
    >
      <LogoMark className="size-[1.3em]" />
      NOALBS
    </span>
  );
}
