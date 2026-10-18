import { useMutation, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useSwitcher } from "@/hooks/use-switcher";
import { errorMessage } from "@/lib/errors";
import { cn } from "@/lib/utils";

/** The OBS WebSocket port and password the browser source uses (Switcher settings and Setup). */
export function ObsConnectionForm({ className }: { className?: string }) {
  const { workspace, switcher, canManage, path } = useSwitcher();
  const queryClient = useQueryClient();
  const [port, setPort] = useState(String(switcher.obs.port));
  const [password, setPassword] = useState("");

  const save = useMutation({
    mutationFn: (change: { clear_password?: boolean }) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/obs", {
          params: { path },
          body: {
            port: Number(port),
            password: password === "" ? null : password,
            clear_password: change.clear_password ?? false,
          },
        }),
      ),
    onSuccess: (updated) => {
      queryClient.setQueryData(queryKeys.switcher(workspace.id, switcher.id), updated);
      setPassword("");
      toast.success("Saved");
    },
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    save.mutate({});
  };

  const changed = port !== String(switcher.obs.port) || password !== "";

  return (
    <form onSubmit={submit} className={cn("flex flex-col gap-4", className)}>
      <div className="grid gap-4 sm:grid-cols-[8rem_minmax(0,1fr)]">
        <div className="flex flex-col gap-2">
          <Label htmlFor="obs-port">Port</Label>
          <Input
            id="obs-port"
            type="number"
            min={1}
            max={65535}
            value={port}
            onChange={(event) => setPort(event.target.value)}
            disabled={!canManage}
            className="font-mono tabular"
          />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="obs-password">Password</Label>
          <Input
            id="obs-password"
            type="password"
            autoComplete="new-password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            placeholder={switcher.obs.has_password ? "Saved — type to replace" : "No password"}
            disabled={!canManage}
          />
        </div>
      </div>
      <p className="text-xs text-muted-foreground">
        The password is stored encrypted and only ever sent to your browser source.
      </p>
      {save.error && <p className="text-sm text-destructive">{errorMessage(save.error)}</p>}
      {canManage && (
        <div className="flex flex-wrap gap-2">
          <Button type="submit" disabled={!changed || save.isPending}>
            {save.isPending ? "Saving…" : "Save"}
          </Button>
          {switcher.obs.has_password && (
            <Button
              type="button"
              variant="outline"
              onClick={() => save.mutate({ clear_password: true })}
              disabled={save.isPending}
            >
              Remove password
            </Button>
          )}
        </div>
      )}
    </form>
  );
}
