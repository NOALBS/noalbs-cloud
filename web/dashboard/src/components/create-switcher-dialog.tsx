import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { type FormEvent, type ReactNode, useState } from "react";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import type { Workspace } from "@/api/types";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { errorMessage, isApiError } from "@/lib/errors";

/** Asks for a name, creates the switcher and opens its setup page. */
export function CreateSwitcherDialog({
  workspace,
  children,
}: {
  workspace: Workspace;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const create = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers", {
          params: { path: { workspace_id: workspace.id } },
          body: { name },
        }),
      ),
    onSuccess: async (created) => {
      // The URL holds the connect token and is only returned now; keep it for
      // the setup page until the tab is closed.
      queryClient.setQueryData(
        queryKeys.connectLink(created.switcher.id),
        created.browser_source_url,
      );
      queryClient.setQueryData(
        queryKeys.switcher(workspace.id, created.switcher.id),
        created.switcher,
      );
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.switchers(workspace.id) }),
        queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
        queryClient.invalidateQueries({ queryKey: queryKeys.workspacePlan(workspace.id) }),
      ]);
      setOpen(false);
      setName("");
      await navigate({
        to: "/$workspace/s/$switcher/setup",
        params: { workspace: workspace.url_slug, switcher: created.switcher.id },
      });
    },
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    create.mutate();
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) create.reset();
      }}
    >
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <form onSubmit={submit} className="flex flex-col gap-5">
          <DialogHeader>
            <DialogTitle>Add a switcher</DialogTitle>
            <DialogDescription>
              A switcher connects one OBS to your stream servers. Most streamers need one.
            </DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-2">
            <Label htmlFor="switcher-name">Name</Label>
            <Input
              id="switcher-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="Main switcher"
              maxLength={60}
              autoFocus
            />
          </div>
          {create.error && (
            <p className="text-sm text-destructive">
              {errorMessage(create.error)}{" "}
              {isApiError(create.error, "plan_limit") && (
                <Link
                  to="/$workspace/settings/plan"
                  params={{ workspace: workspace.url_slug }}
                  className="font-semibold underline"
                  onClick={() => setOpen(false)}
                >
                  See plans
                </Link>
              )}
            </p>
          )}
          <DialogFooter>
            <Button type="submit" disabled={!name.trim() || create.isPending}>
              {create.isPending ? "Adding…" : "Add switcher"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
