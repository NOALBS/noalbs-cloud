import { t } from "@lingui/core/macro";
import type { ConnectAccess, PcObsState, Watchdog } from "@/api/types";
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

export function watchdogOptions(): {
  value: Watchdog;
  label: string;
  description: string;
}[] {
  return [
    {
      value: "off",
      label: t`Off`,
      description: t`If OBS crashes or stops responding on a paired computer, Connect only reports it. It doesn’t restart OBS.`,
    },
    {
      value: "restart",
      label: t`Restart OBS`,
      description: t`If OBS crashes or stops responding on a paired computer, Connect restarts it. It doesn’t go live again on its own.`,
    },
    {
      value: "restart_resume",
      label: t`Restart OBS and go live again`,
      description: t`If OBS crashes or stops responding on a paired computer, Connect restarts it and starts the stream again if it was live.`,
    },
  ];
}

export function pcObsLabel(state: PcObsState): string {
  return {
    unknown: t`OBS state unknown`,
    not_running: t`OBS not running`,
    starting: t`OBS starting`,
    connected: t`OBS connected`,
    not_responding: t`OBS not responding`,
    waiting_for_input: t`OBS is waiting for input`,
    safe_mode: t`OBS is in safe mode`,
    crashed: t`OBS crashed`,
    websocket_disabled: t`OBS WebSocket is disabled`,
  }[state];
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

export function connectObsActionAvailability(state: PcObsState | undefined) {
  return {
    visible: state !== undefined,
    canStart: state === "not_running" || state === "crashed",
    canStopOrRestart: state !== undefined && state !== "not_running" && state !== "unknown",
  };
}

export function canShowConnectObsControls(
  level: ConnectAccess,
  actor: Actor,
  state: PcObsState | undefined,
): boolean {
  return connectObsActionAvailability(state).visible && canControlConnect(level, actor);
}
