import { useMutation, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import { ToggleRow } from "@/components/settings";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useSwitcher } from "@/hooks/use-switcher";
import { errorMessage } from "@/lib/errors";

/**
 * The BELABOX Cloud remote key (write-only, like the OBS password) and whether
 * BELABOX starts and/or stops with the OBS stream. Optional: without a key nothing
 * BELABOX-related runs or shows.
 */
export function BelaboxSettings() {
  const { workspace, switcher, canManage, path } = useSwitcher();
  const queryClient = useQueryClient();
  const [key, setKey] = useState("");
  const {
    has_remote_key: hasKey,
    start_with_stream: startWithStream,
    stop_with_stream: stopWithStream,
  } = switcher.belabox;

  const save = useMutation({
    mutationFn: (change: {
      remote_key?: string;
      clear_remote_key?: boolean;
      start_with_stream?: boolean;
      stop_with_stream?: boolean;
    }) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/belabox", {
          params: { path },
          body: {
            remote_key: change.remote_key ?? null,
            clear_remote_key: change.clear_remote_key ?? false,
            start_with_stream: change.start_with_stream ?? startWithStream,
            stop_with_stream: change.stop_with_stream ?? stopWithStream,
          },
        }),
      ),
    onSuccess: (updated, change) => {
      queryClient.setQueryData(queryKeys.switcher(workspace.id, switcher.id), updated);
      if (change.remote_key !== undefined) setKey("");
      toast.success(change.clear_remote_key ? "BELABOX disconnected" : "Saved");
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    save.mutate({ remote_key: key.trim() });
  };

  return (
    <div className="flex flex-col">
      <form onSubmit={submit} className="flex flex-col gap-3 px-5 pb-5">
        <div className="flex flex-col gap-2">
          <Label htmlFor="belabox-key">BELABOX remote secret URL</Label>
          <Input
            id="belabox-key"
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={key}
            onChange={(event) => setKey(event.target.value)}
            placeholder={hasKey ? "Saved — paste to replace" : "https://remote.belabox.net/?key=…"}
            disabled={!canManage}
            className="font-mono"
          />
        </div>
        <p className="text-xs text-muted-foreground">
          Copy the <strong>BELABOX remote secret URL</strong> from your{" "}
          <a
            href="https://cloud.belabox.net"
            target="_blank"
            rel="noreferrer"
            className="text-primary hover:underline"
          >
            BELABOX Cloud
          </a>{" "}
          account and paste it here. It’s stored encrypted and never shown again. Anyone with it can
          control your BELABOX.
        </p>
        {canManage && (
          <div className="flex flex-wrap gap-2">
            <Button type="submit" disabled={key.trim() === "" || save.isPending}>
              {save.isPending ? "Saving…" : hasKey ? "Replace secret URL" : "Connect BELABOX"}
            </Button>
            {hasKey && (
              <Button
                type="button"
                variant="outline"
                onClick={() =>
                  save.mutate({
                    clear_remote_key: true,
                    start_with_stream: false,
                    stop_with_stream: false,
                  })
                }
                disabled={save.isPending}
              >
                Disconnect
              </Button>
            )}
          </div>
        )}
      </form>
      <ToggleRow
        id="belabox-start-with-stream"
        title="Start BELABOX with the OBS stream"
        description="When the OBS stream starts (from OBS, the dashboard or !start), BELABOX starts with its saved settings."
        checked={startWithStream}
        onCheckedChange={(checked) => save.mutate({ start_with_stream: checked })}
        disabled={!canManage || !hasKey || save.isPending}
        note={hasKey ? undefined : "Needs the secret URL"}
      />
      <ToggleRow
        id="belabox-stop-with-stream"
        title="Stop BELABOX with the OBS stream"
        description="When the OBS stream stops (from OBS, the dashboard, !stop or a raid), BELABOX stops too. Leave off to keep it running."
        checked={stopWithStream}
        onCheckedChange={(checked) => save.mutate({ stop_with_stream: checked })}
        disabled={!canManage || !hasKey || save.isPending}
        note={hasKey ? undefined : "Needs the secret URL"}
      />
      <p className="border-t border-divider px-5 py-3 text-xs text-muted-foreground">
        NOALBS Cloud works with BELABOX Cloud but is not affiliated with or endorsed by BELABOX.
        BELABOX is a trademark of its owner.
      </p>
    </div>
  );
}
