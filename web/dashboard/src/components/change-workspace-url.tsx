import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { meQuery, queryKeys, workspaceBySlugQuery } from "@/api/queries";
import { SlugField, useSlugStatus } from "@/components/slug-field";
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
import { useWorkspace } from "@/hooks/use-workspace";
import { errorMessage } from "@/lib/errors";

/**
 * Moves the workspace to another address (`/old-url/...` → `/new-url/...`). The
 * old one stops working at once and is free for anyone; nothing else hangs on it
 * (OBS and Connect reach the workspace by their own links).
 */
export function ChangeWorkspaceUrl() {
  const { workspace } = useWorkspace();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const [typed, setTyped] = useState(workspace.url_slug);
  const unchanged = typed === workspace.url_slug;
  // Its own URL is "taken" (by itself), so only another one is checked.
  const slug = useSlugStatus("", typed, !unchanged);

  const change = useMutation({
    mutationFn: () =>
      unwrap(
        api.PATCH("/workspaces/{workspace_id}", {
          params: { path: { workspace_id: workspace.id } },
          body: { url_slug: slug.slug },
        }),
      ),
    onSuccess: async (updated) => {
      // This page is at the new address now. Go there before anything re-reads the
      // old one, which is a 404 from here on.
      queryClient.setQueryData(workspaceBySlugQuery(updated.url_slug).queryKey, updated);
      queryClient.setQueryData(meQuery.queryKey, (current) =>
        current?.last_workspace_slug === workspace.url_slug
          ? { ...current, last_workspace_slug: updated.url_slug }
          : current,
      );
      setOpen(false);
      await navigate({
        to: "/$workspace/settings/general",
        params: { workspace: updated.url_slug },
        replace: true,
      });
      // Forget the old address, or going Back to it would show the workspace from
      // the cache (a refetch that fails keeps the data it had).
      queryClient.removeQueries({
        queryKey: workspaceBySlugQuery(workspace.url_slug).queryKey,
        exact: true,
      });
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
        // An organization lists its clients by their URL.
        queryClient.invalidateQueries({ queryKey: queryKeys.organizations }),
      ]);
      toast.success(t`Workspace URL changed`);
    },
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    // The dialog is a portal, but React still bubbles its events to the page.
    event.stopPropagation();
    change.mutate();
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) {
          setTyped(workspace.url_slug);
          change.reset();
        }
      }}
    >
      <DialogTrigger asChild>
        <Button type="button" variant="outline" size="sm">
          <Trans>Change</Trans>
        </Button>
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>
              <Trans>Change the workspace URL</Trans>
            </DialogTitle>
            <DialogDescription>
              <Trans>
                The old address stops working straight away and someone else can take it, so
                bookmarks and links to this workspace need updating. OBS browser sources keep
                working.
              </Trans>
            </DialogDescription>
          </DialogHeader>
          <SlugField status={slug} onChange={setTyped} />
          {change.error && <p className="text-sm text-destructive">{errorMessage(change.error)}</p>}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              <Trans>Cancel</Trans>
            </Button>
            <Button type="submit" disabled={unchanged || !slug.ready || change.isPending}>
              {change.isPending ? t`Saving…` : t`Change URL`}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
