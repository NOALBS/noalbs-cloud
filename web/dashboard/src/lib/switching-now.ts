import { t } from "@lingui/core/macro";
import type {
  Behaviour,
  SceneRule,
  Scenes,
  ServerSample,
  StreamServer,
  SwitcherStatus,
  Triggers,
} from "@/api/types";
import { ruleLabel } from "@/lib/scene-rules";

type Input = {
  status: SwitcherStatus | null | undefined;
  triggers: Triggers;
  behaviour: Pick<Behaviour, "only_switch_when_streaming" | "switch_to_live">;
  scenes: Scenes;
  servers: Pick<StreamServer, "id" | "name" | "enabled" | "overrides">[];
  rules: SceneRule[];
  samples: ServerSample[];
  assignmentsKnown?: boolean;
};
export function switchingNow({
  status,
  triggers,
  behaviour,
  scenes,
  servers,
  rules,
  samples,
  assignmentsKnown = true,
}: Input): { text: string; state?: "live" | "low" | "offline" } {
  if (status === undefined) return { text: t`Waiting for switcher status.` };
  if (!status) return { text: t`The switcher is offline.` };
  if (!status.obs_connected) return { text: t`OBS is not connected.` };
  if (status.paused) return { text: t`This switcher is paused and is not switching scenes.` };
  if (!status.auto_switch)
    return {
      text: status.streaming
        ? t`Auto switch is off; OBS stays on the scene you choose.`
        : t`OBS is not streaming and auto switch is off.`,
    };
  if (!status.streaming)
    return {
      text: behaviour.only_switch_when_streaming
        ? t`OBS is not streaming; auto switching takes over when the stream starts.`
        : t`OBS is not streaming; auto switching is enabled.`,
    };
  const uuid = status.current_scene?.uuid;
  const assigned = [
    scenes.live,
    scenes.low,
    scenes.offline,
    ...servers.flatMap((server) => Object.values(server.overrides)),
    ...rules.flatMap((rule) => Object.values(rule.scenes)),
  ];
  if (
    assignmentsKnown &&
    uuid &&
    !assigned.includes(uuid) &&
    !(behaviour.switch_to_live && uuid === scenes.starting) &&
    !status.active_scene_rule_id
  )
    return {
      text: t`The current scene is not assigned to NOALBS’s automatic scenes; it may be under manual control.`,
    };
  const state = status.state === "idle" ? undefined : status.state;
  if (state === "offline") return { state, text: t`No server has a usable feed.` };
  const server = servers.find((server) => server.id === status.active_server_id && server.enabled);
  const sample = server && samples.find((sample) => sample.server_id === server.id);
  if (state === "low" && server && sample) {
    if (triggers.low != null && sample.bitrate <= triggers.low)
      return {
        state,
        text: t`${server.name} is at or below the LOW bitrate trigger of ${triggers.low} kbps.`,
      };
    if (triggers.rtt != null && sample.rtt != null && sample.rtt >= triggers.rtt)
      return {
        state,
        text: t`${server.name} is at or above the LOW RTT trigger of ${triggers.rtt} ms.`,
      };
  }
  if (
    state === "live" &&
    server &&
    sample &&
    sample.state === "live" &&
    (triggers.low == null || sample.bitrate > triggers.low) &&
    (triggers.rtt == null || sample.rtt == null || sample.rtt < triggers.rtt)
  ) {
    const rule = rules.find((rule) => rule.id === status.active_scene_rule_id);
    const combination = rule && ruleLabel(rule, servers);
    if (rule && combination && rule.scenes.live && rule.scenes.live === uuid)
      return {
        state,
        text: t`${server.name} is above the LOW triggers, so the combination ${combination} picks the scene.`,
      };
    return { state, text: t`${server.name} is above the LOW triggers.` };
  }
  if (state === "low")
    return {
      state,
      text: t`NOALBS reports a LOW feed; no current reading identifies the trigger.`,
    };
  if (state === "live") return { state, text: t`NOALBS reports a LIVE feed.` };
  return { text: t`Waiting for the current feed state.` };
}
