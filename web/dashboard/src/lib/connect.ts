import { t } from "@lingui/core/macro";
import type { ConnectAccess, Watchdog } from "@/api/types";
import { type Actor, isOwner, isSupport } from "./permissions";

/** The roles who can control OBS through Connect or view Connect logs. */
export function connectAccessOptions(
  managedBy: string | null,
): { value: ConnectAccess; label: string; description: string }[] {
  return [
    {
      value: "owners",
      label: managedBy ? t`Only ${managedBy}` : t`Only the owner`,
      description: managedBy
        ? t`${managedBy}’s team can use it. Admins and operators can’t.`
        : t`Only the workspace’s owner can use it. Admins and operators can’t.`,
    },
    {
      value: "admins",
      label: managedBy ? t`${managedBy} and admins` : t`The owner and admins`,
      description: managedBy
        ? t`${managedBy}’s team and workspace admins can use it. Operators can’t.`
        : t`The owner and workspace admins can use it. Operators can’t.`,
    },
    {
      value: "everyone",
      label: t`Everyone in the workspace`,
      description: t`Owners, admins and operators can use it.`,
    },
  ];
}

export function watchdogOptions(): { value: Watchdog; label: string }[] {
  return [
    { value: "off", label: t`Off` },
    { value: "restart", label: t`Restart OBS` },
    { value: "restart_resume", label: t`Restart OBS and go live again` },
  ];
}

/** Mirrors the API's role check for the watchdog setting; never support. */
export function canControlConnect(level: ConnectAccess, actor: Actor): boolean {
  if (isSupport(actor)) return false;
  if (level === "owners") return isOwner(actor);
  if (level === "admins") return isOwner(actor) || actor.role === "admin";
  return (
    isOwner(actor) ||
    actor.role === "admin" ||
    actor.role === "operator" ||
    Boolean(actor.organization)
  );
}
