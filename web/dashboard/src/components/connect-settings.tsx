import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Copy, Download } from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import { api, apiWithTimeout, unwrap } from "@/api/client";
import { connectDevicesQuery, queryKeys, switchersQuery } from "@/api/queries";
import type {
  ConnectAccess,
  ConnectDeviceDto,
  ConnectLogDetailDto,
  ConnectLogDto,
  PcObsState,
  Watchdog,
} from "@/api/types";
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
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
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
  canViewConnectLogs,
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
  const logsAllowed = canViewConnectLogs(switcher.connect_logs, actor);
  const obsControlAllowed = watchdogAllowed;
  const watchdogChoices = watchdogOptions();
  const watchdogDescription = watchdogChoices.find(
    (option) => option.value === switcher.connect_watchdog,
  )?.description;
  const logs = useQuery({
    queryKey: [...queryKeys.connectDevices(workspace.id, switcher.id), "logs"],
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/connect/logs", {
          params: { path },
        }),
      ),
    enabled: logsAllowed,
  });
  const onlineDevice = devices.data?.find((device) => device.id === pc?.device_id);
  const [viewingLog, setViewingLog] = useState<ConnectLogDto | null>(null);

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
  const uploadLatestLog = useMutation({
    mutationFn: () =>
      unwrap(
        apiWithTimeout(40_000).POST(
          "/workspaces/{workspace_id}/switchers/{switcher_id}/connect/logs",
          { params: { path } },
        ),
      ),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: [...queryKeys.connectDevices(workspace.id, switcher.id), "logs"],
      });
      toast.success(t`OBS log uploaded`);
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
        {logsAllowed && (
          <section className="flex flex-col gap-3 rounded-lg border border-divider p-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div>
                <h3 className="text-sm font-semibold">
                  <Trans>Logs</Trans>
                </h3>
                <p className="text-sm text-muted-foreground">
                  <Trans>Crash reports and OBS logs uploaded by Connect.</Trans>
                </p>
              </div>
              <Button
                variant="outline"
                size="sm"
                disabled={!onlineDevice || uploadLatestLog.isPending}
                onClick={() => uploadLatestLog.mutate()}
              >
                <Trans>Get latest OBS log</Trans>
              </Button>
            </div>
            {logs.isPending && (
              <p className="text-sm text-muted-foreground">
                <Trans>Loading logs…</Trans>
              </p>
            )}
            {logs.error && <p className="text-sm text-destructive">{errorMessage(logs.error)}</p>}
            {logs.data?.length === 0 && (
              <p className="text-sm text-muted-foreground">
                <Trans>
                  No logs yet. Connect uploads one after an OBS crash, or get the latest one here.
                </Trans>
              </p>
            )}
            {logs.data?.map((log) => (
              <ConnectLogRow
                key={log.id}
                log={log}
                path={path}
                onView={() => setViewingLog(log)}
                onDeleted={() => {
                  void queryClient.invalidateQueries({
                    queryKey: [...queryKeys.connectDevices(workspace.id, switcher.id), "logs"],
                  });
                }}
              />
            ))}
          </section>
        )}
        <ConnectLogViewer
          log={viewingLog}
          path={path}
          open={viewingLog !== null}
          onOpenChange={(open) => !open && setViewingLog(null)}
        />
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

function ConnectLogRow({
  log,
  path,
  onView,
  onDeleted,
}: {
  log: ConnectLogDto;
  path: { workspace_id: string; switcher_id: string };
  onView: () => void;
  onDeleted: () => void;
}) {
  const remove = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE("/workspaces/{workspace_id}/switchers/{switcher_id}/connect/logs/{log_id}", {
          params: { path: { ...path, log_id: log.id } },
        }),
      ),
    onSuccess: onDeleted,
    onError: (error) => toast.error(errorMessage(error)),
  });
  return (
    <div className="flex flex-wrap items-center gap-3 rounded-lg border border-divider p-3">
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2 text-sm font-medium">
          <span className="rounded bg-muted px-1.5 py-0.5 text-xs">
            {log.kind === "obs_log" ? t`OBS log` : t`Crash report`}
          </span>
          <span className="truncate text-muted-foreground">{log.file_name}</span>
          {log.truncated && (
            <span className="rounded bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
              <Trans>Truncated</Trans>
            </span>
          )}
        </div>
        <p className="text-xs text-muted-foreground">
          {formatRelative(log.uploaded_at)} · {formatBytes(log.size_bytes)} ·{" "}
          {log.device_name ?? t`Device removed`}
        </p>
      </div>
      <Button variant="outline" size="sm" onClick={onView}>
        <Trans>View</Trans>
      </Button>
      <Button
        variant="outline"
        size="sm"
        onClick={() =>
          void downloadLog(path, log).catch((error) => toast.error(errorMessage(error)))
        }
      >
        <Download className="size-4" />
        <Trans>Download .txt</Trans>
      </Button>
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button variant="outline" size="sm">
            <Trans>Delete</Trans>
          </Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              <Trans>Delete this log?</Trans>
            </AlertDialogTitle>
            <AlertDialogDescription>
              <Trans>This uploaded log will be permanently deleted.</Trans>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={remove.isPending}>
              <Trans>Cancel</Trans>
            </AlertDialogCancel>
            <AlertDialogAction disabled={remove.isPending} onClick={() => remove.mutate()}>
              <Trans>Delete</Trans>
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function ConnectLogViewer({
  log,
  path,
  open,
  onOpenChange,
}: {
  log: ConnectLogDto | null;
  path: { workspace_id: string; switcher_id: string };
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [filter, setFilter] = useState("");
  const [copied, setCopied] = useState(false);
  const detail = useQuery({
    queryKey: [...queryKeys.connectDevices(path.workspace_id, path.switcher_id), "log", log?.id],
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/connect/logs/{log_id}", {
          params: { path: { ...path, log_id: log?.id ?? "" } },
        }),
      ),
    enabled: open && log !== null,
  });
  const content = (detail.data as ConnectLogDetailDto | undefined)?.content ?? "";
  const lines = useMemo(() => {
    const normalized = filter.trim().toLocaleLowerCase();
    return content
      .split("\n")
      .map((text, index) => ({ text, index }))
      .filter(({ text }) => !normalized || text.toLocaleLowerCase().includes(normalized));
  }, [content, filter]);
  const copy = async () => {
    await navigator.clipboard.writeText(content);
    setCopied(true);
    toast.success(t`Copied`);
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex h-[min(52rem,90dvh)] flex-col gap-3 sm:max-w-5xl">
        <DialogHeader>
          <DialogTitle>{log?.file_name ?? t`OBS log`}</DialogTitle>
        </DialogHeader>
        <div className="flex gap-2">
          <Input
            aria-label={t`Filter log lines`}
            placeholder={t`Filter log lines`}
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
          />
          <Button variant="outline" onClick={() => void copy()} disabled={!content}>
            {copied ? <Check className="size-4" /> : <Copy className="size-4" />}
            {copied ? t`Copied` : t`Copy`}
          </Button>
        </div>
        {detail.isPending && (
          <p className="text-sm text-muted-foreground">
            <Trans>Loading log…</Trans>
          </p>
        )}
        {detail.error && <p className="text-sm text-destructive">{errorMessage(detail.error)}</p>}
        <pre className="min-h-0 flex-1 overflow-auto rounded-md bg-muted p-3 font-mono text-xs leading-relaxed">
          {lines.map(({ text, index }) => (
            <div key={index}>
              {filter && text.toLocaleLowerCase().includes(filter.trim().toLocaleLowerCase()) ? (
                <mark>{text}</mark>
              ) : (
                text
              )}
            </div>
          ))}
        </pre>
      </DialogContent>
    </Dialog>
  );
}

async function downloadLog(
  path: { workspace_id: string; switcher_id: string },
  log: ConnectLogDto,
) {
  const detail = await unwrap(
    api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/connect/logs/{log_id}", {
      params: { path: { ...path, log_id: log.id } },
    }),
  );
  const url = URL.createObjectURL(new Blob([detail.content], { type: "text/plain;charset=utf-8" }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = `${log.file_name.replace(/\.txt$/i, "")}.txt`;
  anchor.click();
  URL.revokeObjectURL(url);
}

function formatBytes(size: number) {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}
