import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { connectDevicesQuery, queryKeys } from "@/api/queries";
import type { ConnectAccess, ConnectDeviceDto, Watchdog } from "@/api/types";
import { Section } from "@/components/page";
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
import { useSwitcher } from "@/hooks/use-switcher";
import { canControlConnect, connectAccessOptions, watchdogOptions } from "@/lib/connect";
import { errorMessage } from "@/lib/errors";
import { isOwner, isSupport } from "@/lib/permissions";
import { formatRelative } from "@/lib/time";

export function ConnectSettings() {
  const { workspace, switcher, actor, canManage, path } = useSwitcher();
  const queryClient = useQueryClient();
  const devices = useQuery(connectDevicesQuery(workspace.id, switcher.id));
  const owner = isOwner(actor);
  const canManageDevices = canManage && !isSupport(actor);
  const accessOptions = connectAccessOptions(workspace.managed_by?.name ?? null);
  const watchdogAllowed = canControlConnect(switcher.connect_obs_control, actor);

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
              {watchdogOptions().map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
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
}: {
  device: ConnectDeviceDto;
  path: { workspace_id: string; switcher_id: string };
  canManage: boolean;
}) {
  const queryClient = useQueryClient();
  const [name, setName] = useState(device.name);
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
      toast.success(t`Device disconnected`);
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
          {device.last_seen_at
            ? t`Last seen ${formatRelative(device.last_seen_at)}`
            : t`Not connected yet`}
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
                  <Trans>Disconnect {device.name}?</Trans>
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
                  <Trans>Disconnect</Trans>
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </div>
      )}
    </div>
  );
}
