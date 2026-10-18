import {
  closestCenter,
  DndContext,
  type DragEndEvent,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";
import { GripVertical, Info, Plus, Server } from "lucide-react";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { obsScenesQuery, queryKeys, streamServersQuery, workspacePlanQuery } from "@/api/queries";
import type { StreamServer, StreamServerKind } from "@/api/types";
import { ScenePicker } from "@/components/scene-picker";
import { SecretUrlInput } from "@/components/secret-url-input";
import {
  type DetailsForm,
  detailsForm,
  detailsInput,
  ServerDetailsFields,
  sameDetails,
} from "@/components/server-details-fields";
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { useCreateStreamServer } from "@/hooks/use-create-stream-server";
import { DESKTOP, useMediaQuery } from "@/hooks/use-media-query";
import { useSwitcher } from "@/hooks/use-switcher";
import { errorMessage } from "@/lib/errors";
import { formatKbps } from "@/lib/live-format";
import { SCENE_ROLES } from "@/lib/scene-roles";
import { maskUrl } from "@/lib/secret-url";
import { kindInfo, STREAM_SERVER_KINDS } from "@/lib/stream-server-kinds";
import { cn } from "@/lib/utils";
import { isStale, latest } from "@/live/reducer";
import { useSwitcherLive } from "@/live/use-switcher-live";

export const Route = createFileRoute("/_app/$workspace/s/$switcher/servers")({
  component: ServersPage,
});

type Path = { workspace_id: string; switcher_id: string };

/** What a server is doing right now, for its row. */
interface Reading {
  text: string;
  tone: "live" | "idle" | "off";
}

function ServersPage() {
  const { workspace, switcher, canManage, path } = useSwitcher();
  const queryClient = useQueryClient();
  const key = queryKeys.streamServers(workspace.id, switcher.id);
  const servers = useQuery(streamServersQuery(workspace.id, switcher.id));
  const plan = useQuery({ ...workspacePlanQuery(workspace.id), enabled: canManage });
  const maxServers = plan.data?.plan.max_stream_servers;
  const full = maxServers != null && (servers.data?.length ?? 0) >= maxServers;
  const live = useSwitcherLive(workspace.id, switcher.id);
  const desktop = useMediaQuery(DESKTOP);
  const [adding, setAdding] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  // On phones the editor opens as a dialog instead of the side pane.
  const [dialogOpen, setDialogOpen] = useState(false);

  const selected =
    servers.data?.find((server) => server.id === selectedId) ?? servers.data?.[0] ?? null;
  const status = live.status && !isStale(live.status) ? live.status : null;

  const readingOf = (server: StreamServer): Reading => {
    if (!server.enabled) return { text: "Disabled", tone: "off" };
    const sample = status?.obs_connected ? latest(live, server.id) : undefined;
    if (!sample || (sample.bitrate === 0 && sample.state === "offline"))
      return { text: status?.obs_connected ? "No feed" : "Waiting for OBS", tone: "idle" };
    const parts = [`${formatKbps(sample.bitrate)} kbps`];
    if (sample.rtt != null) parts.push(`${Math.round(sample.rtt)} ms`);
    const inUse = server.id === status?.active_server_id;
    if (inUse) parts.push("in use");
    return { text: parts.join(" · "), tone: inUse ? "live" : "idle" };
  };

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const reorder = useMutation({
    mutationFn: (ids: string[]) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers/order", {
          params: { path },
          body: { ids },
        }),
      ),
    onMutate: async (ids) => {
      await queryClient.cancelQueries({ queryKey: key });
      const previous = queryClient.getQueryData<StreamServer[]>(key);
      queryClient.setQueryData<StreamServer[]>(key, (current) =>
        ids
          .map((id) => current?.find((server) => server.id === id))
          .filter((server): server is StreamServer => server !== undefined),
      );
      return { previous };
    },
    onError: (error, _, context) => {
      queryClient.setQueryData(key, context?.previous);
      toast.error(errorMessage(error));
    },
    onSuccess: (ordered) => queryClient.setQueryData(key, ordered),
  });

  const onDragEnd = ({ active, over }: DragEndEvent) => {
    const list = servers.data;
    if (!list || !over || active.id === over.id) return;
    const from = list.findIndex((server) => server.id === active.id);
    const to = list.findIndex((server) => server.id === over.id);
    reorder.mutate(arrayMove(list, from, to).map((server) => server.id));
  };

  const select = (id: string) => {
    setSelectedId(id);
    if (!desktop) setDialogOpen(true);
  };

  const editor = selected && (
    <ServerEditor
      key={selected.id}
      server={selected}
      path={path}
      canManage={canManage}
      onRemoved={() => {
        setSelectedId(null);
        setDialogOpen(false);
      }}
    />
  );

  return (
    <SwitcherPage
      title="Stream servers"
      description="Where your encoder sends video. NOALBS uses the first one in the list that is receiving."
      backToSettings
      actions={
        canManage && (
          <Button onClick={() => setAdding(true)} disabled={full}>
            <Plus />
            Add server
          </Button>
        )
      }
    >
      {!canManage && <ReadOnlyNotice />}
      {canManage && full && (
        <ReadOnlyNotice>
          The {plan.data?.plan.name} plan allows {maxServers}{" "}
          {maxServers === 1 ? "stream server" : "stream servers"} per switcher.{" "}
          <Link
            to="/$workspace/settings/plan"
            params={{ workspace: workspace.url_slug }}
            className="font-medium text-primary hover:underline"
          >
            See plans
          </Link>
        </ReadOnlyNotice>
      )}

      {servers.isPending && <Skeleton className="h-20 rounded-2xl" />}

      {servers.data?.length === 0 && (
        <div className="flex flex-col items-center gap-3 rounded-2xl border border-dashed px-6 py-10 text-center">
          <span className="flex size-11 items-center justify-center rounded-xl bg-primary-soft text-primary-soft-foreground">
            <Server className="size-5" />
          </span>
          <p className="font-semibold">No stream servers yet</p>
          <p className="max-w-sm text-sm text-muted-foreground">
            Add the server your encoder streams to, such as BELABOX Cloud or your own MediaMTX.
          </p>
        </div>
      )}

      {servers.data && servers.data.length > 0 && (
        <div className="grid grid-cols-1 items-start gap-4 lg:grid-cols-12">
          <section aria-label="Server order" className="flex flex-col gap-2.5 lg:col-span-5">
            <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
              <SortableContext
                items={servers.data.map((server) => server.id)}
                strategy={verticalListSortingStrategy}
              >
                <ol className="flex flex-col gap-2.5">
                  {servers.data.map((server, index) => (
                    <ServerRow
                      key={server.id}
                      server={server}
                      position={index + 1}
                      reading={readingOf(server)}
                      selected={desktop && server.id === selected?.id}
                      canManage={canManage}
                      path={path}
                      onSelect={() => select(server.id)}
                    />
                  ))}
                </ol>
              </SortableContext>
            </DndContext>
            <p className="flex gap-2 pt-1 text-[13px] leading-relaxed text-muted-foreground">
              <Info className="mt-0.5 size-4 shrink-0" />
              {servers.data.length === 1
                ? "Tip: add a second server as a backup uplink so NOALBS can fail over."
                : "Each server can override the LIVE, LOW and BRB scenes, e.g. a different overlay for a backup uplink."}
            </p>
          </section>

          {desktop && (
            <section
              aria-label="Server details"
              className="flex flex-col rounded-2xl border bg-card lg:col-span-7"
            >
              {editor}
            </section>
          )}
        </div>
      )}

      {!desktop && (
        <Dialog open={dialogOpen && selected != null} onOpenChange={setDialogOpen}>
          <DialogContent className="max-h-[90dvh] gap-0 overflow-y-auto p-0">
            <DialogHeader className="sr-only">
              <DialogTitle>{selected?.name}</DialogTitle>
              <DialogDescription>Change how NOALBS reaches this server.</DialogDescription>
            </DialogHeader>
            {editor}
          </DialogContent>
        </Dialog>
      )}

      {canManage && (
        <AddServerDialog
          open={adding}
          onOpenChange={setAdding}
          path={path}
          onAdded={(id) => setSelectedId(id)}
        />
      )}
    </SwitcherPage>
  );
}

function ServerRow({
  server,
  position,
  reading,
  selected,
  canManage,
  path,
  onSelect,
}: {
  server: StreamServer;
  position: number;
  reading: Reading;
  selected: boolean;
  canManage: boolean;
  path: Path;
  onSelect: () => void;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: server.id, disabled: !canManage });
  const toggle = useToggleServer(server, path);
  const info = kindInfo(server.kind);

  return (
    <li
      ref={setNodeRef}
      style={{
        transform: transform ? `translate3d(${transform.x}px, ${transform.y}px, 0)` : undefined,
        transition,
      }}
      className={cn(
        "flex items-stretch rounded-2xl border bg-card",
        selected && "border-2 border-primary bg-primary-soft",
        isDragging && "relative z-10 border-primary shadow-2xl",
      )}
    >
      {canManage && (
        <button
          type="button"
          ref={setActivatorNodeRef}
          {...attributes}
          {...listeners}
          aria-label={`Move ${server.name}`}
          className="flex w-9 shrink-0 cursor-grab touch-none items-center justify-center rounded-l-2xl text-subtle-foreground hover:text-foreground active:cursor-grabbing"
        >
          <GripVertical className="size-4" />
        </button>
      )}
      <button
        type="button"
        onClick={onSelect}
        aria-current={selected || undefined}
        className={cn(
          "flex min-w-0 flex-1 flex-col gap-2 py-3.5 pr-4 text-left",
          !canManage && "pl-4",
        )}
      >
        <span className="flex items-center justify-between gap-2">
          <span className="flex min-w-0 items-center gap-2.5">
            <span className="font-mono text-xs text-muted-foreground tabular">
              {String(position).padStart(2, "0")}
            </span>
            <span className="truncate text-[15px] font-semibold">{server.name}</span>
          </span>
          <KindBadge label={info.label} />
        </span>
        <span
          className={cn(
            "flex items-center gap-2 text-[13px]",
            reading.tone === "live" && "text-live",
            reading.tone === "idle" && "text-muted-foreground",
            reading.tone === "off" && "text-subtle-foreground",
          )}
        >
          <span
            className={cn(
              "size-1.5 rounded-full",
              reading.tone === "live"
                ? "bg-live"
                : reading.tone === "idle"
                  ? "bg-idle"
                  : "bg-border",
            )}
          />
          <span className="font-mono tabular">{reading.text}</span>
        </span>
      </button>
      {canManage && (
        <span className="flex items-center pr-4 lg:hidden">
          <Switch
            checked={server.enabled}
            onCheckedChange={(enabled) => toggle.mutate(enabled)}
            disabled={toggle.isPending}
            aria-label={`Use ${server.name}`}
          />
        </span>
      )}
    </li>
  );
}

function KindBadge({ label }: { label: string }) {
  return (
    <Badge className="bg-primary-soft text-[11px] tracking-wider text-primary-soft-foreground uppercase">
      {label}
    </Badge>
  );
}

function useToggleServer(server: StreamServer, path: Path) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (enabled: boolean) =>
      unwrap(
        api.PATCH(
          "/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers/{stream_server_id}",
          {
            params: { path: { ...path, stream_server_id: server.id } },
            body: { enabled },
          },
        ),
      ),
    onSuccess: () =>
      queryClient.invalidateQueries({
        queryKey: queryKeys.streamServers(path.workspace_id, path.switcher_id),
      }),
    onError: (error) => toast.error(errorMessage(error)),
  });
}

/** One server's settings: name, stats URL with a connection test, scene overrides. */
function ServerEditor({
  server,
  path,
  canManage,
  onRemoved,
}: {
  server: StreamServer;
  path: Path;
  canManage: boolean;
  onRemoved: () => void;
}) {
  const queryClient = useQueryClient();
  const [name, setName] = useState(server.name);
  const [statsUrl, setStatsUrl] = useState(server.stats_url ?? "");
  const [overrides, setOverrides] = useState(server.overrides);
  const savedDetails = detailsForm(server.details);
  const [details, setDetails] = useState(savedDetails);
  const detailsChanged = !sameDetails(details, savedDetails);
  const obs = useQuery(obsScenesQuery(path.workspace_id, path.switcher_id));
  const toggle = useToggleServer(server, path);
  const info = kindInfo(server.kind);
  const serverPath = { ...path, stream_server_id: server.id };
  const dirty =
    name !== server.name ||
    statsUrl !== (server.stats_url ?? "") ||
    JSON.stringify(overrides) !== JSON.stringify(server.overrides) ||
    detailsChanged;

  const refresh = () =>
    Promise.all([
      queryClient.invalidateQueries({
        queryKey: queryKeys.streamServers(path.workspace_id, path.switcher_id),
      }),
      queryClient.invalidateQueries({ queryKey: queryKeys.switchers(path.workspace_id) }),
    ]);

  const update = useMutation({
    mutationFn: async () => {
      await unwrap(
        api.PATCH(
          "/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers/{stream_server_id}",
          {
            params: { path: serverPath },
            body: {
              name,
              stats_url: statsUrl,
              // Only when changed, so stored secrets stay as they are.
              ...(detailsChanged ? { details: detailsInput(details) } : {}),
            },
          },
        ),
      );
      if (JSON.stringify(overrides) !== JSON.stringify(server.overrides)) {
        await unwrap(
          api.PUT(
            "/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers/{stream_server_id}/overrides",
            { params: { path: serverPath }, body: overrides },
          ),
        );
      }
    },
    onSuccess: async () => {
      await refresh();
      // Typed secrets are saved now; start the fields over.
      setDetails((current) => ({
        ...current,
        password: "",
        clear_password: false,
        api_key: "",
        clear_api_key: false,
      }));
      toast.success("Saved");
    },
  });

  const remove = useMutation({
    mutationFn: () =>
      unwrap(
        api.DELETE(
          "/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers/{stream_server_id}",
          { params: { path: serverPath } },
        ),
      ),
    onSuccess: async () => {
      onRemoved();
      await refresh();
      toast.success(`Removed ${server.name}`);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    update.mutate();
  };

  return (
    <form onSubmit={submit} className="flex flex-1 flex-col">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-divider py-4 pr-12 pl-5 lg:px-5.5">
        <span className="flex min-w-0 items-center gap-2.5">
          <span className="truncate text-lg font-semibold">{server.name}</span>
          <KindBadge label={info.label} />
        </span>
        {canManage && (
          <span className="flex items-center gap-2.5 text-sm font-medium">
            {server.enabled ? "Enabled" : "Disabled"}
            <Switch
              checked={server.enabled}
              onCheckedChange={(enabled) => toggle.mutate(enabled)}
              disabled={toggle.isPending}
              aria-label={`Use ${server.name}`}
            />
          </span>
        )}
      </div>

      <div className="flex flex-col gap-5 px-5 py-5 lg:px-5.5">
        <div className="flex flex-col gap-2">
          <Label htmlFor="server-editor-name">Name</Label>
          <Input
            id="server-editor-name"
            value={name}
            onChange={(event) => setName(event.target.value)}
            maxLength={60}
            disabled={!canManage}
          />
        </div>
        {/* Managers, and NOALBS support read-only (to spot a mistyped URL). */}
        {server.stats_url !== null && (
          <div className="flex flex-col gap-2">
            <Label htmlFor="server-editor-url">Stats URL</Label>
            <SecretUrlInput
              id="server-editor-url"
              value={statsUrl}
              onChange={setStatsUrl}
              label="stats URL"
              placeholder={info.placeholder}
              readOnly={!canManage}
            />
            {info.help && <p className="text-xs text-muted-foreground">{info.help}</p>}
          </div>
        )}

        {server.details != null && (
          <ServerDetailsFields
            kind={server.kind}
            form={details}
            onChange={setDetails}
            stored={server.details}
            idPrefix="server-editor"
            disabled={!canManage}
          />
        )}

        {canManage && (
          <ConnectionTest server={server} statsUrl={statsUrl} details={details} path={path} />
        )}

        <fieldset className="flex flex-col gap-2.5">
          <legend className="text-[15px] font-semibold">Scene overrides</legend>
          <p className="text-[13px] text-muted-foreground">
            {obs.isSuccess
              ? "Use different scenes while this server is the one in use."
              : "Connect OBS to choose scenes for this server."}
          </p>
          <div className="grid gap-2.5 sm:grid-cols-3">
            {(["live", "low", "offline"] as const).map((role) => (
              <div key={role} className="flex flex-col gap-1.5">
                <span className="flex items-center gap-1.5 text-[13px] font-semibold">
                  <span className={cn("size-2 rounded-full", SCENE_ROLES[role].dot)} />
                  {SCENE_ROLES[role].label}
                </span>
                <ScenePicker
                  label={`${SCENE_ROLES[role].label} override`}
                  value={overrides[role] ?? null}
                  obs={obs.data}
                  disabled={!canManage || !obs.isSuccess}
                  noneLabel="Use default"
                  onChange={(uuid) => setOverrides({ ...overrides, [role]: uuid })}
                  className="sm:w-full"
                />
              </div>
            ))}
          </div>
        </fieldset>

        {update.error && <p className="text-sm text-destructive">{errorMessage(update.error)}</p>}
      </div>

      {canManage && (
        <div className="mt-auto flex items-center justify-between gap-3 border-t border-divider px-5 py-4 lg:px-5.5">
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button
                type="button"
                variant="outline"
                className="border-destructive/50 text-destructive-foreground"
              >
                Remove server
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Remove {server.name}?</AlertDialogTitle>
                <AlertDialogDescription>
                  NOALBS stops reading this server’s stats.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction variant="destructive" onClick={() => remove.mutate()}>
                  Remove
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
          <Button
            type="submit"
            disabled={!dirty || !name.trim() || !statsUrl.trim() || update.isPending}
          >
            {update.isPending ? "Saving…" : "Save server"}
          </Button>
        </div>
      )}
    </form>
  );
}

/**
 * Fetches the stats URL through the API and shows what came back. Runs on its
 * own for enabled servers; "Test" checks the URL as typed, before saving.
 */
function ConnectionTest({
  server,
  statsUrl,
  details,
  path,
}: {
  server: StreamServer;
  statsUrl: string;
  /** The kind-specific settings as typed; stored secrets fill in the rest. */
  details: DetailsForm;
  path: Path;
}) {
  const [tested, setTested] = useState({
    url: server.stats_url ?? "",
    details: detailsInput(details),
    asked: server.enabled,
  });
  const probe = useQuery({
    queryKey: ["stats-probe", server.id, server.kind, tested.url, tested.details],
    queryFn: () =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers/test", {
          params: { path },
          body: {
            kind: server.kind,
            stats_url: tested.url,
            details: tested.details,
            stream_server_id: server.id,
          },
        }),
      ),
    enabled: tested.asked && tested.url.trim() !== "",
    staleTime: 30_000,
    retry: false,
  });

  const test = () => {
    const now = { url: statsUrl, details: detailsInput(details), asked: true };
    if (JSON.stringify(now) === JSON.stringify(tested)) void probe.refetch();
    else setTested(now);
  };

  const result = probe.data;
  const checking = probe.isFetching;
  const ok = !checking && result?.reachable === true;
  const failed = !checking && (result?.reachable === false || probe.isError);

  let title = "Not checked";
  let detail = server.enabled ? "Checks the stats URL from NOALBS Cloud." : "Server is disabled";
  if (checking) {
    title = "Checking…";
    detail = maskUrl(tested.url);
  } else if (probe.isError) {
    title = "Couldn’t test";
    detail = errorMessage(probe.error);
  } else if (result) {
    title = result.reachable ? (result.receiving ? "Receiving" : "Reachable") : "Not reachable";
    const facts = [
      result.http_status != null && `HTTP ${result.http_status}`,
      result.elapsed_ms != null && `in ${result.elapsed_ms} ms`,
      result.bitrate != null && `bitrate ${formatKbps(result.bitrate)} kbps`,
      result.rtt != null && `rtt ${Math.round(result.rtt)} ms`,
    ].filter(Boolean);
    detail = [facts.join(" · "), result.message].filter(Boolean).join(" · ");
  }

  return (
    <div
      role="status"
      aria-label="Connection test"
      className={cn(
        "flex items-center justify-between gap-3 rounded-xl border px-4 py-3.5",
        ok && "border-live/25 bg-live/7",
        failed && "border-offline/35 bg-offline/7",
        !ok && !failed && "border-divider bg-sunken",
      )}
    >
      <span className="flex min-w-0 flex-col gap-1">
        <span
          className={cn(
            "flex items-center gap-2 text-sm font-semibold",
            ok ? "text-live" : failed ? "text-offline" : "text-muted-foreground",
          )}
        >
          <span
            className={cn(
              "size-2 rounded-full",
              ok ? "bg-live" : failed ? "bg-offline" : "bg-idle",
            )}
          />
          {title}
        </span>
        <span className="font-mono text-xs break-words text-muted-foreground">{detail}</span>
      </span>
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={test}
        disabled={checking || !statsUrl.trim()}
      >
        {result || probe.isError ? "Test again" : "Test"}
      </Button>
    </div>
  );
}

function AddServerDialog({
  open,
  onOpenChange,
  path,
  onAdded,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  path: Path;
  onAdded: (id: string) => void;
}) {
  const [kind, setKind] = useState<StreamServerKind>("belabox");
  const [name, setName] = useState("BELABOX Cloud");
  const [nameTouched, setNameTouched] = useState(false);
  const [statsUrl, setStatsUrl] = useState("");
  const [details, setDetails] = useState(detailsForm());
  const info = kindInfo(kind);

  const reset = () => {
    setKind("belabox");
    setName("BELABOX Cloud");
    setNameTouched(false);
    setStatsUrl("");
    setDetails(detailsForm());
  };

  const create = useCreateStreamServer(path);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    create.mutate(
      { name, kind, stats_url: statsUrl, details: detailsInput(details) },
      {
        onSuccess: (server) => {
          onAdded(server.id);
          onOpenChange(false);
          reset();
        },
      },
    );
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        onOpenChange(next);
        if (!next) create.reset();
      }}
    >
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <form onSubmit={submit} className="flex flex-col gap-5">
          <DialogHeader>
            <DialogTitle>Add a stream server</DialogTitle>
            <DialogDescription>Pick the server your encoder streams to.</DialogDescription>
          </DialogHeader>

          <fieldset className="grid grid-cols-2 gap-2 sm:grid-cols-3">
            <legend className="sr-only">Server type</legend>
            {STREAM_SERVER_KINDS.map((option) => (
              <label
                key={option.kind}
                className="flex min-h-15 cursor-pointer flex-col items-center justify-center gap-0.5 rounded-xl border px-2 text-center hover:bg-surface-2 has-checked:border-2 has-checked:border-primary has-checked:bg-primary-soft has-focus-visible:ring-2 has-focus-visible:ring-ring"
              >
                <input
                  type="radio"
                  name="server-kind"
                  value={option.kind}
                  checked={kind === option.kind}
                  onChange={() => {
                    setKind(option.kind);
                    setDetails(detailsForm());
                    if (!nameTouched)
                      setName(option.kind === "belabox" ? "BELABOX Cloud" : option.label);
                  }}
                  className="sr-only"
                />
                <span className="text-sm font-semibold">{option.label}</span>
                <span className="text-[11px] text-muted-foreground">{option.blurb}</span>
              </label>
            ))}
          </fieldset>

          <div className="flex flex-col gap-2">
            <Label htmlFor="server-name">Name</Label>
            <Input
              id="server-name"
              value={name}
              onChange={(event) => {
                setName(event.target.value);
                setNameTouched(true);
              }}
              maxLength={60}
            />
          </div>
          <div className="flex flex-col gap-2">
            <Label htmlFor="server-stats-url">Stats URL</Label>
            <Input
              id="server-stats-url"
              value={statsUrl}
              onChange={(event) => setStatsUrl(event.target.value)}
              placeholder={info.placeholder}
              className="font-mono"
              spellCheck={false}
            />
            <p className="text-xs text-muted-foreground">{info.help}</p>
          </div>

          <ServerDetailsFields
            kind={kind}
            form={details}
            onChange={setDetails}
            idPrefix="new-server"
          />

          {create.error && <p className="text-sm text-destructive">{errorMessage(create.error)}</p>}

          <DialogFooter>
            <Button type="submit" disabled={!name.trim() || !statsUrl.trim() || create.isPending}>
              {create.isPending ? "Adding…" : "Add server"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
