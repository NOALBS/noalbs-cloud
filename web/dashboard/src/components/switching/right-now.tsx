import { t } from "@lingui/core/macro";
import { useQuery } from "@tanstack/react-query";
import { sceneRulesQuery, streamServersQuery } from "@/api/queries";
import { Badge } from "@/components/ui/badge";
import { useNow } from "@/hooks/use-now";
import { useSwitcher } from "@/hooks/use-switcher";
import { formatKbps } from "@/lib/live-format";
import { SCENE_ROLES } from "@/lib/scene-roles";
import { switchingNow } from "@/lib/switching-now";
import { isStale, latest } from "@/live/reducer";
import { useSwitcherLive } from "@/live/use-switcher-live";
export function RightNow() {
  const { workspace, switcher } = useSwitcher();
  const live = useSwitcherLive(workspace.id, switcher.id),
    now = useNow();
  const servers = useQuery(streamServersQuery(workspace.id, switcher.id));
  const rules = useQuery(sceneRulesQuery(workspace.id, switcher.id));
  const status = live.status && isStale(live.status, now) ? null : live.status;
  const samples = (servers.data ?? []).flatMap((server) => {
    const sample = latest(live, server.id);
    return sample && now - sample.at < 15000 ? [{ ...sample, server_id: server.id }] : [];
  });
  const explanation = switchingNow({
    status,
    triggers: switcher.triggers,
    behaviour: switcher.behaviour,
    scenes: switcher.scenes,
    servers: servers.data ?? [],
    rules: rules.data ?? [],
    assignmentsKnown: servers.isSuccess && rules.isSuccess,
    samples,
  });
  const reading = status?.obs_connected
    ? samples.find((sample) => sample.server_id === status.active_server_id)
    : undefined;
  return (
    <section
      aria-label={t`Right now`}
      className="flex flex-wrap items-center justify-between gap-4 rounded-2xl border bg-card px-5 py-4 shadow-tile"
    >
      <div className="flex min-w-0 flex-1 items-center gap-3">
        {explanation.state && (
          <Badge className={`bg-muted ${SCENE_ROLES[explanation.state].text}`}>
            {SCENE_ROLES[explanation.state].label}
          </Badge>
        )}
        <div>
          <h2 className="font-display text-sm font-bold">
            {status?.obs_connected && status.current_scene
              ? t`${status.current_scene.name} is on air`
              : t`Right now`}
          </h2>
          <p className="text-sm text-muted-foreground">{explanation.text}</p>
        </div>
      </div>
      {reading && (
        <div className="flex gap-5">
          <div>
            <p className="font-display text-xl font-bold">{formatKbps(reading.bitrate)}</p>
            <p className="text-xs text-muted-foreground">{t`kbps`}</p>
          </div>
          {reading.rtt != null && (
            <div>
              <p className="font-display text-xl font-bold">{Math.round(reading.rtt)}</p>
              <p className="text-xs text-muted-foreground">{t`ms RTT`}</p>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
