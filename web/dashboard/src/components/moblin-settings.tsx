import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import { CopyField } from "@/components/copy-field";
import { ToggleRow } from "@/components/settings";
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
import { useSwitcher } from "@/hooks/use-switcher";
import { errorMessage } from "@/lib/errors";
import { MOBLIN_MIN_PASSWORD } from "@/lib/moblin";

interface MoblinChange {
  password?: string;
  generate_password?: boolean;
  clear_password?: boolean;
  start_with_stream?: boolean;
  stop_with_stream?: boolean;
}

/**
 * Moblin's remote control: the Assistant URL and password to enter in Moblin
 * (the password is generated here, or your own), and whether Moblin starts and/or
 * stops with the OBS stream. Optional: without a password nothing Moblin-related
 * runs or shows.
 */
export function MoblinSettings() {
  const { workspace, switcher, canManage, canReadSecrets, path } = useSwitcher();
  const queryClient = useQueryClient();
  const {
    has_password: hasPassword,
    start_with_stream: startWithStream,
    stop_with_stream: stopWithStream,
  } = switcher.moblin;

  const save = useMutation({
    mutationFn: (change: MoblinChange) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/moblin", {
          params: { path },
          body: {
            password: change.password ?? null,
            generate_password: change.generate_password ?? false,
            clear_password: change.clear_password ?? false,
            start_with_stream: change.start_with_stream ?? startWithStream,
            stop_with_stream: change.stop_with_stream ?? stopWithStream,
          },
        }),
      ),
    onSuccess: (updated, change) => {
      queryClient.setQueryData(queryKeys.switcher(workspace.id, switcher.id), updated);
      void queryClient.invalidateQueries({
        queryKey: queryKeys.moblinConnection(workspace.id, switcher.id),
      });
      toast.success(
        change.clear_password
          ? t`Moblin disconnected`
          : change.generate_password || change.password !== undefined
            ? t`Password saved. Enter it in Moblin.`
            : t`Saved`,
      );
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <div className="flex flex-col">
      <div className="flex flex-col gap-3 px-5 pb-5">
        {hasPassword ? (
          canManage &&
          (canReadSecrets ? (
            <ConnectionDetails save={save.mutate} saving={save.isPending} />
          ) : (
            <p className="text-sm text-muted-foreground">
              <Trans>Moblin is set up. Its password and URL are hidden from NOALBS support.</Trans>
            </p>
          ))
        ) : (
          <>
            <p className="text-sm text-muted-foreground">
              <Trans>
                Moblin connects to NOALBS Cloud as a remote control “streamer”. Set it up to get a
                password and the URL to enter in Moblin.
              </Trans>
            </p>
            {canManage && (
              <div className="flex flex-wrap gap-2">
                <Button
                  onClick={() => save.mutate({ generate_password: true })}
                  disabled={save.isPending}
                >
                  {save.isPending ? t`Setting up…` : t`Set up Moblin`}
                </Button>
                <OwnPassword save={save.mutate} saving={save.isPending} />
              </div>
            )}
          </>
        )}
      </div>
      <ToggleRow
        id="moblin-start-with-stream"
        title={t`Start Moblin with the OBS stream`}
        description={t`When the OBS stream starts (from OBS, the dashboard or !start), Moblin goes live.`}
        checked={startWithStream}
        onCheckedChange={(checked) => save.mutate({ start_with_stream: checked })}
        disabled={!canManage || !hasPassword || save.isPending}
        note={hasPassword ? undefined : t`Needs Moblin set up`}
      />
      <ToggleRow
        id="moblin-stop-with-stream"
        title={t`Stop Moblin with the OBS stream`}
        description={t`When the OBS stream stops (from OBS, the dashboard, !stop or a raid), Moblin stops too. Leave off to keep it running.`}
        checked={stopWithStream}
        onCheckedChange={(checked) => save.mutate({ stop_with_stream: checked })}
        disabled={!canManage || !hasPassword || save.isPending}
        note={hasPassword ? undefined : t`Needs Moblin set up`}
      />
    </div>
  );
}

/** The steps in Moblin, with the URL and password to copy, and ways to change them. */
function ConnectionDetails({
  save,
  saving,
}: {
  save: (change: MoblinChange) => void;
  saving: boolean;
}) {
  const { workspace, switcher, path } = useSwitcher();
  const connection = useQuery({
    queryKey: queryKeys.moblinConnection(workspace.id, switcher.id),
    queryFn: () =>
      unwrap(
        api.GET("/workspaces/{workspace_id}/switchers/{switcher_id}/moblin/connection", {
          params: { path },
        }),
      ),
  });

  return (
    <>
      <ol className="flex list-decimal flex-col gap-1 pl-5 text-sm text-muted-foreground">
        <li>
          <Trans>
            In Moblin, open <strong>Settings → Remote control</strong> and set the{" "}
            <strong>Password</strong> to:
          </Trans>
        </li>
      </ol>
      {connection.data ? (
        connection.data.password && (
          <CopyField value={connection.data.password} label={t`Moblin password`} secret />
        )
      ) : (
        <p className="text-sm text-muted-foreground">
          {connection.error ? errorMessage(connection.error) : t`Loading…`}
        </p>
      )}
      <ol start={2} className="flex list-decimal flex-col gap-1 pl-5 text-sm text-muted-foreground">
        <li>
          <Trans>
            Open <strong>Streamer → Assistant</strong> and enter this URL:
          </Trans>
        </li>
      </ol>
      {connection.data && <CopyField value={connection.data.url} label={t`Moblin Assistant URL`} />}
      <ol start={3} className="flex list-decimal flex-col gap-1 pl-5 text-sm text-muted-foreground">
        <li>
          <Trans>
            Turn <strong>Enabled</strong> on under <strong>Streamer</strong>.
          </Trans>
        </li>
      </ol>
      <p className="text-xs text-muted-foreground">
        <Trans>
          Moblin uses one remote control password, so if it already has one you use elsewhere, enter
          that here instead. A new password disconnects the Moblin connected now.
        </Trans>
      </p>
      <div className="flex flex-wrap gap-2">
        <AlertDialog>
          <AlertDialogTrigger asChild>
            <Button variant="outline" disabled={saving}>
              <Trans>New password</Trans>
            </Button>
          </AlertDialogTrigger>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>
                <Trans>Generate a new Moblin password?</Trans>
              </AlertDialogTitle>
              <AlertDialogDescription>
                <Trans>Moblin disconnects until you enter the new password in it.</Trans>
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>
                <Trans>Cancel</Trans>
              </AlertDialogCancel>
              <AlertDialogAction onClick={() => save({ generate_password: true })}>
                <Trans>Generate</Trans>
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
        <OwnPassword save={save} saving={saving} />
        <Button
          variant="outline"
          onClick={() =>
            save({ clear_password: true, start_with_stream: false, stop_with_stream: false })
          }
          disabled={saving}
        >
          <Trans>Disconnect</Trans>
        </Button>
      </div>
    </>
  );
}

/** "Use my own password": a small form, opened on demand. */
function OwnPassword({ save, saving }: { save: (change: MoblinChange) => void; saving: boolean }) {
  const [open, setOpen] = useState(false);
  const [password, setPassword] = useState("");
  const valid = password.trim().length >= MOBLIN_MIN_PASSWORD;

  if (!open) {
    return (
      <Button variant="ghost" onClick={() => setOpen(true)}>
        <Trans>Use my own password</Trans>
      </Button>
    );
  }

  const submit = (event: FormEvent) => {
    event.preventDefault();
    save({ password: password.trim() });
    setPassword("");
    setOpen(false);
  };

  return (
    <form onSubmit={submit} className="flex w-full flex-col gap-2">
      <Label htmlFor="moblin-password">
        <Trans>Moblin’s remote control password</Trans>
      </Label>
      <div className="flex gap-2">
        <Input
          id="moblin-password"
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={password}
          onChange={(event) => setPassword(event.target.value)}
          placeholder={t`At least ${MOBLIN_MIN_PASSWORD} characters`}
          className="font-mono"
        />
        <Button type="submit" disabled={!valid || saving}>
          <Trans>Save</Trans>
        </Button>
        <Button type="button" variant="ghost" onClick={() => setOpen(false)}>
          <Trans>Cancel</Trans>
        </Button>
      </div>
    </form>
  );
}
