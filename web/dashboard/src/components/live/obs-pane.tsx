import type { PcObsState } from "@/api/types";
import { ConnectDialogPrograms, ConnectObsButtons } from "@/components/connect-controls";
import { pcObsLabel } from "@/lib/connect";
import { ObsStatsBody } from "./obs-stats-popover";
import { Pane, PaneState } from "./pane";

type Path = { workspace_id: string; switcher_id: string };

/**
 * OBS's stats as a pane. With NOALBS Cloud Connect it also starts, stops and
 * restarts OBS and the computer's programs.
 */
export function ObsPane({
  path,
  connected,
  connect,
  streaming,
  canControlObs,
  canControlPrograms,
}: {
  path: Path;
  connected: boolean;
  /** The Connect computer, when the switcher uses Connect. */
  connect: { deviceName: string; obs: PcObsState } | undefined;
  streaming: boolean;
  canControlObs: boolean;
  canControlPrograms: boolean;
}) {
  return (
    <Pane
      title="OBS"
      label="OBS"
      hint={connect?.deviceName}
      tools={
        <PaneState tone={connected ? "text-live" : "text-offline"}>
          {connected ? "Connected" : "Not connected"}
        </PaneState>
      }
      className="flex flex-col gap-4 [&>div:first-child]:p-0"
    >
      {connected ? (
        <ObsStatsBody path={path} />
      ) : (
        <p className="text-sm text-muted-foreground">
          {connect
            ? `${pcObsLabel(connect.obs)}.`
            : "Open OBS with the NOALBS browser source active to see its stats."}
        </p>
      )}
      {connect && canControlObs && (
        <div className="border-t pt-4">
          <ConnectObsButtons
            path={path}
            obs={connect.obs}
            deviceName={connect.deviceName}
            streaming={streaming}
          />
        </div>
      )}
      {connect && canControlPrograms && <ConnectDialogPrograms path={path} />}
      {connect && !canControlObs && !canControlPrograms && (
        <p className="text-sm text-muted-foreground">
          You don’t have access to control this computer.
        </p>
      )}
    </Pane>
  );
}
