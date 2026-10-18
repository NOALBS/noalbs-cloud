import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import type { SwitcherSummary } from "@/api/types";
import { BelaboxSettings } from "@/components/belabox-settings";
import { CopyField } from "@/components/copy-field";
import { MoblinSettings } from "@/components/moblin-settings";
import { ObsConnectionForm } from "@/components/obs-connection-form";
import { Section } from "@/components/page";
import { ReadOnlyNotice } from "@/components/settings";
import { SwitcherPage } from "@/components/switcher-page";
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

export const Route = createFileRoute("/_app/$workspace/s/$switcher/obs")({
  component: ObsPage,
});

function ObsPage() {
  const { workspace, switcher, canManage } = useSwitcher();

  return (
    <SwitcherPage
      title="Switcher settings"
      description="The OBS connection, the browser-source link, BELABOX Cloud, Moblin and this switcher’s name."
      backToSettings
      className="max-w-3xl"
    >
      {!canManage && <ReadOnlyNotice />}
      <ObsConnection />
      {canManage && <BrowserSourceLink />}
      <Section
        title="BELABOX Cloud"
        description="Optional. Connect your BELABOX to see its modems on the Live page and control it from here and chat (!bbstart, !bbstop, !bbb, !bbt, !bbs)."
      >
        <BelaboxSettings />
      </Section>
      <Section
        title="Moblin"
        description="Optional. Connect Moblin's remote control to see its bitrate, bonding and battery on the Live page and control it from here and chat (!mstart, !mstop, !mbitrate, !mscene, !mstats)."
      >
        <MoblinSettings />
      </Section>
      <Section
        title="Setup guide"
        description="The step-by-step checklist for connecting OBS, a stream server and your scenes."
        actions={
          <Button asChild variant="outline" size="sm">
            <Link
              to="/$workspace/s/$switcher/setup"
              params={{ workspace: workspace.url_slug, switcher: switcher.id }}
            >
              Open setup guide
            </Link>
          </Button>
        }
      />
      <SwitcherName />
      {canManage && <DeleteSwitcher />}
    </SwitcherPage>
  );
}

function ObsConnection() {
  return (
    <Section
      title="OBS WebSocket"
      description="How the browser source reaches OBS on your computer. Match OBS’s Tools → WebSocket Server Settings."
    >
      <ObsConnectionForm className="px-5 pb-5" />
    </Section>
  );
}

function BrowserSourceLink() {
  const { switcher, path } = useSwitcher();
  const queryClient = useQueryClient();
  const { data: url } = useQuery<string | null>({
    queryKey: queryKeys.connectLink(switcher.id),
    queryFn: () => null,
    staleTime: Number.POSITIVE_INFINITY,
  });

  const rotate = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/connect-token", {
          params: { path },
        }),
      ),
    onSuccess: (link) =>
      queryClient.setQueryData(queryKeys.connectLink(switcher.id), link.browser_source_url),
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title="Browser-source link"
      description="The URL pasted into OBS. It contains a secret, so it’s only shown right after it’s created."
    >
      <div className="flex flex-col gap-3 px-5 pb-5">
        {url && <CopyField value={url} label="browser source URL" secret />}
        <AlertDialog>
          <AlertDialogTrigger asChild>
            <Button variant="outline" className="self-start">
              Create a new link
            </Button>
          </AlertDialogTrigger>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>Create a new browser-source link?</AlertDialogTitle>
              <AlertDialogDescription>
                The current link stops working, so you’ll need to paste the new one into OBS. Do
                this if the link leaked.
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>Cancel</AlertDialogCancel>
              <AlertDialogAction onClick={() => rotate.mutate()}>Create new link</AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </div>
    </Section>
  );
}

function SwitcherName() {
  const { workspace, switcher, canManage, path } = useSwitcher();
  const queryClient = useQueryClient();
  const [name, setName] = useState(switcher.name);

  const rename = useMutation({
    mutationFn: () =>
      unwrap(
        api.PATCH("/workspaces/{workspace_id}/switchers/{switcher_id}", {
          params: { path },
          body: { name },
        }),
      ),
    onSuccess: async (updated) => {
      queryClient.setQueryData(queryKeys.switcher(workspace.id, switcher.id), updated);
      await queryClient.invalidateQueries({ queryKey: queryKeys.switchers(workspace.id) });
      toast.success("Renamed");
    },
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    rename.mutate();
  };

  return (
    <Section title="Switcher">
      <form onSubmit={submit} className="flex max-w-md flex-col gap-2 px-5 pb-5">
        <Label htmlFor="switcher-name">Name</Label>
        <div className="flex gap-2">
          <Input
            id="switcher-name"
            value={name}
            onChange={(event) => setName(event.target.value)}
            maxLength={60}
            disabled={!canManage}
          />
          {canManage && (
            <Button
              type="submit"
              disabled={!name.trim() || name.trim() === switcher.name || rename.isPending}
            >
              Save
            </Button>
          )}
        </div>
        {rename.error && <p className="text-sm text-destructive">{errorMessage(rename.error)}</p>}
      </form>
    </Section>
  );
}

function DeleteSwitcher() {
  const { workspace, switcher, path } = useSwitcher();
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const remove = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE("/workspaces/{workspace_id}/switchers/{switcher_id}", { params: { path } }),
      ),
    onSuccess: async () => {
      // Drop it from the list first: the workspace opens on a remaining switcher.
      queryClient.setQueryData<SwitcherSummary[]>(queryKeys.switchers(workspace.id), (current) =>
        current?.filter((item) => item.id !== switcher.id),
      );
      // Leave before removing its query, so the page doesn't re-render without it.
      await navigate({ to: "/$workspace", params: { workspace: workspace.url_slug } });
      queryClient.removeQueries({ queryKey: queryKeys.switcher(workspace.id, switcher.id) });
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.switchers(workspace.id) }),
        queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
        queryClient.invalidateQueries({ queryKey: queryKeys.workspacePlan(workspace.id) }),
      ]);
      toast.success(`Deleted ${switcher.name}`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  return (
    <Section
      title="Delete switcher"
      description="Removes this switcher and its stream servers. The browser source in OBS stops working."
      className="border-destructive/40"
    >
      <div className="px-5 pb-5">
        <AlertDialog>
          <AlertDialogTrigger asChild>
            <Button variant="outline" className="border-destructive/50 text-destructive-foreground">
              Delete {switcher.name}
            </Button>
          </AlertDialogTrigger>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>Delete {switcher.name}?</AlertDialogTitle>
              <AlertDialogDescription>
                This can’t be undone. Its settings and stream servers are deleted too.
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>Cancel</AlertDialogCancel>
              <AlertDialogAction variant="destructive" onClick={() => remove.mutate()}>
                Delete
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </div>
    </Section>
  );
}
