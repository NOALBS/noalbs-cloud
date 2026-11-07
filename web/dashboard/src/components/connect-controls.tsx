import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Play, RotateCw, Square } from "lucide-react";
import { type ReactElement, useState } from "react";
import { toast } from "sonner";
import { api, apiWithTimeout, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import type { PcObsState } from "@/api/types";
import { StatusDot } from "@/components/switcher-status";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { useConnectObsCommand } from "@/hooks/use-connect-obs-command";
import { connectObsActionAvailability, pcObsLabel } from "@/lib/connect";
import { errorMessage } from "@/lib/errors";

type Path = { workspace_id: string; switcher_id: string };

export type ConnectProgramDto = {
  id: string;
  name: string;
  path: string;
  args: string[];
  start_with_obs: boolean;
  stop_with_obs: boolean;
  close_when_idle: boolean;
  added_from_dashboard_by: string | null;
};
export type ConnectProgramStatusDto = { program: ConnectProgramDto; running: boolean };

export function connectProgramsKey(path: Path) {
  return [...queryKeys.connectDevices(path.workspace_id, path.switcher_id), "programs"];
}

/** The paired computer's programs and whether each is running, refreshed every 2s. */
export function useConnectPrograms(path: Path) {
  return useQuery({
    queryKey: connectProgramsKey(path),
    queryFn: async () =>
      (await unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/connect/programs", {
          params: { path },
        }),
      )) as ConnectProgramStatusDto[],
    refetchInterval: 2_000,
  });
}

/** Starts or stops one program on the paired computer. */
export function useConnectProgramControl(path: Path, onError?: (error: unknown) => void) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ programId, action }: { programId: string; action: "start" | "stop" }) =>
      unwrap(
        apiWithTimeout(70_000).POST(
          "/workspaces/{workspace_id}/switchers/{switcher_id}/connect/programs/{program_id}/{action}",
          { params: { path: { ...path, program_id: programId, action } } },
        ),
      ),
    onSuccess: () =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: connectProgramsKey(path) }),
        queryClient.invalidateQueries({ queryKey: queryKeys.switchers(path.workspace_id) }),
      ]),
    onError: onError ?? ((error) => toast.error(errorMessage(error))),
  });
}

/** Start or Stop (whichever fits OBS's state) and Restart, asking before stopping OBS. */
export function ConnectObsButtons({
  path,
  obs,
  deviceName,
  streaming,
}: {
  path: Path;
  obs: PcObsState;
  deviceName: string;
  streaming: boolean;
}) {
  const [confirmAction, setConfirmAction] = useState<"stop" | "restart" | null>(null);
  const obsCommand = useConnectObsCommand(path);
  const { canStart, canStopOrRestart } = connectObsActionAvailability(obs);

  return (
    <div className="flex flex-wrap items-center gap-2">
      {/* Start wins when both fit: a crashed OBS needs starting, not stopping. */}
      {!canStart && canStopOrRestart ? (
        <Button
          variant="outline"
          size="sm"
          disabled={obsCommand.isPending}
          onClick={() => setConfirmAction("stop")}
        >
          <Square />
          <Trans>Stop OBS</Trans>
        </Button>
      ) : (
        <Button
          variant="outline"
          size="sm"
          disabled={!canStart || obsCommand.isPending}
          onClick={() => obsCommand.mutate("start")}
        >
          <Play />
          <Trans>Start OBS</Trans>
        </Button>
      )}
      <Button
        variant="outline"
        size="sm"
        disabled={!canStopOrRestart || obsCommand.isPending}
        onClick={() => setConfirmAction("restart")}
      >
        <RotateCw />
        <Trans>Restart OBS</Trans>
      </Button>
      <AlertDialog
        open={confirmAction !== null}
        onOpenChange={(open) => !open && setConfirmAction(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {confirmAction === "stop" ? t`Stop OBS?` : t`Restart OBS?`}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {streaming ? (
                <Trans>
                  The stream is live and will drop if OBS stops. Do you want to continue?
                </Trans>
              ) : confirmAction === "stop" ? (
                <Trans>Stop OBS on {deviceName}?</Trans>
              ) : (
                <Trans>Restart OBS on {deviceName}?</Trans>
              )}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={obsCommand.isPending}>
              <Trans>Cancel</Trans>
            </AlertDialogCancel>
            <AlertDialogAction
              disabled={obsCommand.isPending}
              onClick={(event) => {
                event.preventDefault();
                if (confirmAction) {
                  obsCommand.mutate(confirmAction, {
                    onSuccess: () => setConfirmAction(null),
                  });
                }
              }}
            >
              {confirmAction === "stop" ? <Trans>Stop OBS</Trans> : <Trans>Restart OBS</Trans>}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

/** Start or Stop for one program, whichever fits whether it's running. */
export function ConnectProgramToggle({
  running,
  disabled,
  onToggle,
}: {
  running: boolean;
  disabled?: boolean;
  onToggle: (action: "start" | "stop") => void;
}) {
  return (
    <Button
      size="sm"
      variant="outline"
      disabled={disabled}
      onClick={() => onToggle(running ? "stop" : "start")}
    >
      {running ? <Square /> : <Play />}
      {running ? <Trans>Stop</Trans> : <Trans>Start</Trans>}
    </Button>
  );
}

/**
 * The paired Connect computer's OBS and program controls in a modal opened by
 * `children`, the Connect status chip. Lists programs only while open.
 */
export function ConnectDialog({
  path,
  deviceName,
  obs,
  streaming,
  canControlObs,
  canControlPrograms,
  settingsLink,
  children,
}: {
  path: Path;
  deviceName: string;
  obs: PcObsState;
  streaming: boolean;
  canControlObs: boolean;
  canControlPrograms: boolean;
  settingsLink: ReactElement;
  children: ReactElement;
}) {
  return (
    <Dialog>
      <DialogTrigger asChild>{children}</DialogTrigger>
      {/* Capped to the screen; only the program list scrolls, so OBS stays in view. */}
      <DialogContent className="flex max-h-[88dvh] flex-col">
        <DialogHeader>
          <DialogTitle className="break-words">{deviceName}</DialogTitle>
          <DialogDescription className="flex items-center gap-1.5">
            <StatusDot className="bg-live" />
            <Trans>NOALBS Cloud Connect · {pcObsLabel(obs)}</Trans>
          </DialogDescription>
        </DialogHeader>
        {canControlObs && (
          <section aria-label={t`OBS`} className="flex flex-col gap-2">
            <h3 className="text-sm font-semibold">
              <Trans>OBS</Trans>
            </h3>
            <ConnectObsButtons
              path={path}
              obs={obs}
              deviceName={deviceName}
              streaming={streaming}
            />
          </section>
        )}
        {canControlPrograms && <ConnectDialogPrograms path={path} />}
        {!canControlObs && !canControlPrograms && (
          <p className="text-sm text-muted-foreground">
            <Trans>You don’t have access to control this computer.</Trans>
          </p>
        )}
        <div className="flex justify-end text-sm">
          {/* Close first: the app bar's chip outlives the page change. */}
          <DialogClose asChild>{settingsLink}</DialogClose>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function ConnectDialogPrograms({ path }: { path: Path }) {
  const programs = useConnectPrograms(path);
  const control = useConnectProgramControl(path);

  return (
    <section aria-label={t`Programs`} className="flex min-h-0 flex-col gap-2">
      <h3 className="text-sm font-semibold">
        <Trans>Programs</Trans>
      </h3>
      {programs.error && <p className="text-sm text-destructive">{errorMessage(programs.error)}</p>}
      {programs.isLoading && (
        <p className="text-sm text-muted-foreground">
          <Trans>Loading programs…</Trans>
        </p>
      )}
      {programs.data?.length === 0 && (
        <p className="text-sm text-muted-foreground">
          <Trans>No programs have been added yet.</Trans>
        </p>
      )}
      {programs.data && programs.data.length > 0 && (
        <ul className="flex min-h-0 flex-col divide-y divide-divider overflow-y-auto rounded-md border border-divider">
          {programs.data.map(({ program, running }) => (
            <li key={program.id} className="flex items-center gap-3 px-3 py-2">
              <div className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-sm font-medium">{program.name}</span>
                <span className={running ? "text-xs text-live" : "text-xs text-muted-foreground"}>
                  {running ? t`Running` : t`Stopped`}
                </span>
              </div>
              <ConnectProgramToggle
                running={running}
                disabled={control.isPending}
                onToggle={(action) => control.mutate({ programId: program.id, action })}
              />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
