import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { connectDevicesQuery, queryKeys, switchersQuery } from "@/api/queries";
import type { ConnectAccess, ConnectDeviceDto, PcObsState, Watchdog } from "@/api/types";
import { Section } from "@/components/page";
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
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { type ConnectObsAction, useConnectObsCommand } from "@/hooks/use-connect-obs-command";
import { useSwitcher } from "@/hooks/use-switcher";
import {
  canControlConnect,
  connectAccessOptions,
  connectObsActionAvailability,
  pcObsLabel,
  watchdogOptions,
} from "@/lib/connect";
import { errorMessage } from "@/lib/errors";
import { isOwner, isSupport } from "@/lib/permissions";
import { formatRelative } from "@/lib/time";

export function ConnectSettings() {
  const { workspace, switcher, actor, canManage, path } = useSwitcher();
  const queryClient = useQueryClient();
  const devices = useQuery(connectDevicesQuery(workspace.id, switcher.id));
  const summary = useQuery(switchersQuery(workspace.id)).data?.find(
    (candidate) => candidate.id === switcher.id,
  );
  const pc = summary?.status?.pc;
  const owner = isOwner(actor);
  const canManageDevices = canManage && !isSupport(actor);
  const accessOptions = connectAccessOptions(workspace.managed_by?.name ?? null);
  const watchdogAllowed = canControlConnect(switcher.connect_obs_control, actor);
  const obsControlAllowed = watchdogAllowed;
  const watchdogChoices = watchdogOptions();
  const watchdogDescription = watchdogChoices.find(
    (option) => option.value === switcher.connect_watchdog,
  )?.description;

  const savePermissions = useMutation({
    mutationFn: (change: Partial<{ obs_control: ConnectAccess; logs: ConnectAccess }>) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/connect/permissions", {
          params: { path },
          body: {
            obs_control: change.obs_control ?? switcher.connect_obs_control,
            logs: change.logs ?? switcher.connect_logs,
          },
        }),
      ),
    onSuccess: (updated) => {
      queryClient.setQueryData(queryKeys.switcher(workspace.id, switcher.id), updated);
      toast.success(t`Saved`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const saveWatchdog = useMutation({
    mutationFn: (mode: Watchdog) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/connect/watchdog", {
          params: { path },
          body: { mode },
        }),
      ),
    onSuccess: (updated) => {
      queryClient.setQueryData(queryKeys.switcher(workspace.id, switcher.id), updated);
      toast.success(t`Saved`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title={t`NOALBS Cloud Connect`}
      description={t`Manage computers paired with this switcher and choose who can control OBS.`}
    >
      <div className="flex flex-col gap-5 px-5 pb-5">
        <div className="flex flex-col gap-3">
          <h3 className="text-sm font-semibold">
            <Trans>Paired computers</Trans>
          </h3>
          {devices.isPending && (
            <p className="text-sm text-muted-foreground">
              <Trans>Loading devices…</Trans>
            </p>
          )}
          {devices.error && (
            <p className="text-sm text-destructive">{errorMessage(devices.error)}</p>
          )}
          {devices.data?.length === 0 && (
            <p className="text-sm text-muted-foreground">
              <Trans>No computers are paired yet.</Trans>
            </p>
          )}
          {devices.data?.map((device) => (
            <ConnectDeviceRow
              key={device.id}
              device={device}
              path={path}
              canManage={canManageDevices}
              onlineObs={pc?.device_id === device.id ? pc.obs : undefined}
              canControlObs={obsControlAllowed}
              streaming={Boolean(summary?.status?.streaming)}
            />
          ))}
        </div>

        <div className="grid gap-4 border-t border-divider pt-4 md:grid-cols-2">
          <AccessSelect
            id="connect-obs-control"
            title={t`Who can start, stop and restart OBS`}
            value={switcher.connect_obs_control}
            options={accessOptions}
            disabled={!owner || savePermissions.isPending}
            onChange={(value) => savePermissions.mutate({ obs_control: value })}
          />
          <AccessSelect
            id="connect-logs"
            title={t`Who can see OBS logs`}
            value={switcher.connect_logs}
            options={accessOptions}
            disabled={!owner || savePermissions.isPending}
            onChange={(value) => savePermissions.mutate({ logs: value })}
          />
        </div>
        {!owner && (
          <p className="text-sm text-muted-foreground">
            {workspace.managed_by
              ? t`Only ${workspace.managed_by.name} can change these permissions.`
              : t`Only the workspace’s owner can change these permissions.`}
          </p>
        )}

        <div className="flex max-w-xl flex-col gap-2 border-t border-divider pt-4">
          <Label htmlFor="connect-watchdog">
            <Trans>OBS watchdog</Trans>
          </Label>
          <Select
            value={switcher.connect_watchdog}
            onValueChange={(next) => saveWatchdog.mutate(next as Watchdog)}
            disabled={!watchdogAllowed || saveWatchdog.isPending}
          >
            <SelectTrigger id="connect-watchdog" className="w-full max-w-sm">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {watchdogChoices.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <p className="text-sm text-muted-foreground">{watchdogDescription}</p>
          {!watchdogAllowed && (
            <p className="text-sm text-muted-foreground">
              <Trans>
                Your workspace role can’t control OBS through Connect, so it can’t change the
                watchdog.
              </Trans>
            </p>
          )}
        </div>
      </div>
    </Section>
  );
}

function AccessSelect({
  id,
  title,
  value,
  options,
  disabled,
  onChange,
}: {
  id: string;
  title: string;
  value: ConnectAccess;
  options: ReturnType<typeof connectAccessOptions>;
  disabled: boolean;
  onChange: (value: ConnectAccess) => void;
}) {
  const current = options.find((option) => option.value === value);
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor={id}>{title}</Label>
      <Select
        value={value}
        onValueChange={(next) => onChange(next as ConnectAccess)}
        disabled={disabled}
      >
        <SelectTrigger id={id} className="w-full max-w-sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {options.map((option) => (
            <SelectItem key={option.value} value={option.value}>
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <p className="text-sm text-muted-foreground">{current?.description}</p>
    </div>
  );
}

function ConnectDeviceRow({
  device,
  path,
  canManage,
  onlineObs,
  canControlObs,
  streaming,
}: {
  device: ConnectDeviceDto;
  path: { workspace_id: string; switcher_id: string };
  canManage: boolean;
  onlineObs?: PcObsState;
  canControlObs: boolean;
  streaming: boolean;
}) {
  const queryClient = useQueryClient();
  const [name, setName] = useState(device.name);
  const [confirmAction, setConfirmAction] = useState<"stop" | "restart" | null>(null);
  const obsCommand = useConnectObsCommand(path);
  const obsActions = connectObsActionAvailability(onlineObs);
  const changed = name.trim() !== device.name;
  const rename = useMutation({
    mutationFn: () =>
      unwrap(
        api.PATCH(
          "/workspaces/{workspace_id}/switchers/{switcher_id}/connect/devices/{device_id}",
          {
            params: { path: { ...path, device_id: device.id } },
            body: { name: name.trim() },
          },
        ),
      ),
    onSuccess: (updated) => {
      setName(updated.name);
      queryClient.setQueryData<ConnectDeviceDto[]>(
        queryKeys.connectDevices(path.workspace_id, path.switcher_id),
        (current) => current?.map((item) => (item.id === updated.id ? updated : item)),
      );
      toast.success(t`Renamed`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
  const revoke = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE(
          "/workspaces/{workspace_id}/switchers/{switcher_id}/connect/devices/{device_id}",
          {
            params: { path: { ...path, device_id: device.id } },
          },
        ),
      ),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: queryKeys.connectDevices(path.workspace_id, path.switcher_id),
      });
      toast.success(t`Computer revoked`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <div className="flex flex-col gap-3 rounded-xl border border-divider p-3 sm:flex-row sm:items-center">
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex flex-wrap items-center gap-x-2 text-sm font-semibold">
          <span className="truncate">{device.name}</span>
          <span className="text-xs font-normal text-muted-foreground">
            {device.os} · {device.app_version}
          </span>
        </div>
        <span className="text-xs text-muted-foreground">
          {onlineObs ? (
            <span className="inline-flex items-center gap-1.5">
              <StatusDot className="bg-live" />
              {t`Online · ${pcObsLabel(onlineObs)}`}
            </span>
          ) : device.last_seen_at ? (
            t`Last seen ${formatRelative(device.last_seen_at)}`
          ) : (
            t`Not connected yet`
          )}
        </span>
      </div>
      {canManage && (
        <div className="flex flex-wrap items-center gap-2">
          <Input
            aria-label={t`Rename ${device.name}`}
            value={name}
            maxLength={100}
            onChange={(event) => setName(event.target.value)}
            className="h-9 w-40"
          />
          <Button
            variant="outline"
            size="sm"
            disabled={!name.trim() || !changed || rename.isPending}
            onClick={() => rename.mutate()}
          >
            <Trans>Rename</Trans>
          </Button>
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button variant="outline" size="sm">
                <Trans>Revoke</Trans>
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>
                  <Trans>Revoke {device.name}?</Trans>
                </AlertDialogTitle>
                <AlertDialogDescription>
                  <Trans>
                    This computer’s token will stop working. It must be paired again to reconnect.
                  </Trans>
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>
                  <Trans>Cancel</Trans>
                </AlertDialogCancel>
                <AlertDialogAction onClick={() => revoke.mutate()}>
                  <Trans>Revoke</Trans>
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </div>
      )}
      {canControlObs && obsActions.visible && (
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={!obsActions.canStart || obsCommand.isPending}
            onClick={() => obsCommand.mutate("start")}
          >
            <Trans>Start OBS</Trans>
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={!obsActions.canStopOrRestart || obsCommand.isPending}
            onClick={() => setConfirmAction("stop")}
          >
            <Trans>Stop OBS</Trans>
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={!obsActions.canStopOrRestart || obsCommand.isPending}
            onClick={() => setConfirmAction("restart")}
          >
            <Trans>Restart OBS</Trans>
          </Button>
        </div>
      )}
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
                <Trans>Stop OBS on {device.name}?</Trans>
              ) : (
                <Trans>Restart OBS on {device.name}?</Trans>
              )}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={obsCommand.isPending}>
              <Trans>Cancel</Trans>
            </AlertDialogCancel>
            <AlertDialogAction
              disabled={obsCommand.isPending}
              onClick={() => {
                if (confirmAction) {
                  obsCommand.mutate(confirmAction satisfies ConnectObsAction, {
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
