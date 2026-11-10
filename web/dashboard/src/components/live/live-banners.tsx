import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { MonitorPlay, PauseCircle, PlugZap, WifiOff } from "lucide-react";
import type { SwitcherStatus } from "@/api/types";
import { Button } from "@/components/ui/button";
import type { useConnectObsCommand } from "@/hooks/use-connect-obs-command";
import type { useSwitcher } from "@/hooks/use-switcher";
import { connectObsActionAvailability, pcObsLabel } from "@/lib/connect";

export function LiveBanners({
  workspace,
  switcher,
  paused,
  status,
  feedDown,
  canControlConnectObs,
  obsCommand,
}: Pick<ReturnType<typeof useSwitcher>, "workspace" | "switcher"> & {
  paused: boolean;
  status: SwitcherStatus | null | undefined;
  feedDown: boolean;
  canControlConnectObs: boolean;
  obsCommand: ReturnType<typeof useConnectObsCommand>;
}) {
  const params = { workspace: workspace.url_slug, switcher: switcher.id };
  const connectDeviceName = status?.pc?.device_name ?? t`Connect computer`;
  return (
    <>
      {paused && workspace.suspension && (
        <div className="flex flex-wrap items-center gap-3 rounded-2xl border border-destructive/40 bg-destructive/10 p-4">
          <PauseCircle className="size-5 text-destructive-foreground" />
          <p className="flex-1 text-sm">
            {workspace.suspension.reason ? (
              <Trans>
                <strong>Suspended by NOALBS.</strong> {workspace.suspension.reason} This switcher
                stays connected but doesn't switch or take commands. Questions:{" "}
                <a
                  href="mailto:support@noalbs.com"
                  className="font-semibold text-primary hover:underline"
                >
                  support@noalbs.com
                </a>
              </Trans>
            ) : (
              <>
                <Trans>
                  <strong>This workspace is paused.</strong> This switcher stays connected but
                  doesn't switch or take commands.
                </Trans>
                {workspace.managed_by && (
                  <>
                    {" "}
                    <Trans>Ask {workspace.managed_by.name} about it.</Trans>
                  </>
                )}
              </>
            )}
          </p>
        </div>
      )}
      {paused && !workspace.suspension && (
        <div className="flex flex-wrap items-center gap-3 rounded-2xl border border-low/40 bg-low/10 p-4">
          <PauseCircle className="size-5 text-low" />
          <p className="flex-1 text-sm">
            <Trans>
              <strong>Paused.</strong> This workspace has more switchers than its plan includes, so
              this one stays connected but doesn't switch or take commands. Upgrade the plan, or
              delete a switcher you no longer use.
            </Trans>
          </p>
          <Link
            to="/$workspace/settings/plan"
            params={{ workspace: workspace.url_slug }}
            className="text-sm font-semibold text-primary hover:underline"
          >
            <Trans>See plans</Trans>
          </Link>
        </div>
      )}
      {feedDown && (
        <div
          role="status"
          className="flex flex-wrap items-center gap-3 rounded-2xl border border-low/40 bg-low/10 p-4"
        >
          <WifiOff className="size-5 text-low" />
          <p className="flex-1 text-sm">
            <Trans>
              <strong>Reconnecting to NOALBS…</strong> This page is out of date until it’s back.
              Switching keeps running without it; check your phone’s signal.
            </Trans>
          </p>
        </div>
      )}
      {status === null && !feedDown && (
        <div className="flex flex-wrap items-center gap-3 rounded-2xl border border-dashed p-4">
          <MonitorPlay className="size-5 text-primary" />
          <p className="flex-1 text-sm text-muted-foreground">
            <Trans>
              The browser source isn’t connected. Open OBS with the NOALBS browser source active.
            </Trans>
          </p>
          <Link
            to="/$workspace/s/$switcher/setup"
            params={params}
            className="text-sm font-semibold text-primary hover:underline"
          >
            <Trans>Setup help</Trans>
          </Link>
        </div>
      )}
      {status && !status.obs_connected && !feedDown && (
        <div className="flex flex-col gap-3 rounded-2xl border border-low/40 bg-low/10 p-4 sm:flex-row sm:items-center">
          <div className="flex min-w-0 flex-1 items-start gap-3">
            <PlugZap className="mt-px size-5 shrink-0 text-low" />
            <p className="min-w-0 flex-1 text-sm [overflow-wrap:anywhere]">
              {status.pc ? (
                <Trans>
                  <strong>{connectDeviceName} is connected, but OBS isn’t.</strong>{" "}
                  {pcObsLabel(status.pc.obs)}.
                </Trans>
              ) : (
                <Trans>
                  <strong>The browser source can’t reach OBS.</strong> Check that WebSocket is on in
                  OBS (Tools → WebSocket Server Settings) and that the port and password match
                  Setup.
                </Trans>
              )}
            </p>
          </div>
          <div className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2 pl-8 sm:pl-0">
            {status.pc ? (
              <>
                {canControlConnectObs && connectObsActionAvailability(status.pc.obs).canStart && (
                  <Button
                    size="sm"
                    disabled={obsCommand.isPending}
                    onClick={() => obsCommand.mutate("start")}
                  >
                    <Trans>Start OBS</Trans>
                  </Button>
                )}
                <Link
                  to="/$workspace/s/$switcher/obs"
                  params={params}
                  className="text-sm font-semibold text-primary hover:underline"
                >
                  <Trans>Open settings</Trans>
                </Link>
              </>
            ) : (
              <Link
                to="/$workspace/s/$switcher/setup"
                params={params}
                className="text-sm font-semibold text-primary hover:underline"
              >
                <Trans>Open Setup</Trans>
              </Link>
            )}
          </div>
        </div>
      )}
    </>
  );
}
