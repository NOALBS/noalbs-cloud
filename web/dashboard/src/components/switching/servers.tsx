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
import { t } from "@lingui/core/macro";
import { Plural, Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useSearch } from "@tanstack/react-router";
import { ChevronRight, GripVertical, Plus, Server } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import {
  obsScenesQuery,
  queryKeys,
  sceneRulesQuery,
  streamServersQuery,
  workspacePlanQuery,
} from "@/api/queries";
import type { StreamServer, StreamServerKind } from "@/api/types";
import { DocsLink } from "@/components/docs-link";
import { Section } from "@/components/page";
import { SecretUrlInput } from "@/components/secret-url-input";
import {
  type DetailsForm,
  detailsForm,
  detailsInput,
  ServerDetailsFields,
  sameDetails,
} from "@/components/server-details-fields";
import { ReadOnlyNotice } from "@/components/settings";
import { SceneChoice as ScenePicker } from "@/components/switching/scene-choice";
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
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { useCreateStreamServer } from "@/hooks/use-create-stream-server";
import { useSwitcher } from "@/hooks/use-switcher";
import { errorMessage } from "@/lib/errors";
import { formatKbps } from "@/lib/live-format";
import { SCENE_ROLES } from "@/lib/scene-roles";
import { ruleLabel, rulesWithServer } from "@/lib/scene-rules";
import { maskUrl } from "@/lib/secret-url";
import { kindInfo, streamServerKinds } from "@/lib/stream-server-kinds";
import { cn } from "@/lib/utils";
import { isStale, latest } from "@/live/reducer";
import { useSwitcherLive } from "@/live/use-switcher-live";

type Path = { workspace_id: string; switcher_id: string };

/** What a server is doing right now, for its row. */
interface Reading {
  text: string;
  tone: "live" | "idle" | "off";
}

export function ServersPage() {
  const { workspace, switcher, canManage, path } = useSwitcher();
  const queryClient = useQueryClient();
  const key = queryKeys.streamServers(workspace.id, switcher.id);
  const servers = useQuery(streamServersQuery(workspace.id, switcher.id));
  const plan = useQuery({ ...workspacePlanQuery(workspace.id), enabled: canManage });
  const maxServers = plan.data?.plan.max_stream_servers;
  const full = maxServers != null && (servers.data?.length ?? 0) >= maxServers;
  const live = useSwitcherLive(workspace.id, switcher.id);
  const search = useSearch({ strict: false });
  const [adding, setAdding] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  useEffect(() => {
    if (search.server) setSelectedId(search.server);
  }, [search.server]);

  const selected = servers.data?.find((server) => server.id === selectedId) ?? null;
  const status = live.status && !isStale(live.status) ? live.status : null;

  const readingOf = (server: StreamServer): Reading => {
    if (!server.enabled) return { text: t`Disabled`, tone: "off" };
    const sample = status?.obs_connected ? latest(live, server.id) : undefined;
    if (!sample || (sample.bitrate === 0 && sample.state === "offline"))
      return { text: status?.obs_connected ? t`No feed` : t`Waiting for OBS`, tone: "idle" };
    const parts = [`${formatKbps(sample.bitrate)} kbps`];
    if (sample.rtt != null) parts.push(`${Math.round(sample.rtt)} ms`);
    parts.push(sample.state === "offline" ? t`No feed` : t`receiving`);
    return { text: parts.join(" · "), tone: sample.state === "offline" ? "idle" : "live" };
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
  };

  const editor = selected && (
    <ServerEditor
      key={selected.id}
      server={selected}
      path={path}
      canManage={canManage}
      onRemoved={() => {
        setSelectedId(null);
      }}
    />
  );

  return (
    <Section
      title={t`Servers`}
      description={t`Where your encoder sends video. NOALBS uses the first one in the list that is receiving.`}
      actions={
        canManage && (
          <Button onClick={() => setAdding(true)} disabled={full}>
            <Plus />
            <Trans>Add server</Trans>
          </Button>
        )
      }
    >
      {!canManage && <ReadOnlyNotice />}
      {canManage && full && (
        <ReadOnlyNotice>
          <Trans>
            The {plan.data?.plan.name} plan allows{" "}
            <Plural value={maxServers} one="# stream server" other="# stream servers" /> per
            switcher.{" "}
            <Link
              to="/$workspace/settings/plan"
              params={{ workspace: workspace.url_slug }}
              className="font-medium text-primary hover:underline"
            >
              See plans
            </Link>
          </Trans>
        </ReadOnlyNotice>
      )}

      {servers.isPending && <Skeleton className="h-20 rounded-2xl" />}
      {servers.error && (
        <p className="px-5 py-4 text-sm text-destructive">{errorMessage(servers.error)}</p>
      )}

      {servers.data?.length === 0 && (
        <div className="flex flex-col items-center gap-3 rounded-2xl border border-dashed px-6 py-10 text-center">
          <span className="flex size-11 items-center justify-center rounded-xl bg-primary-soft text-primary-soft-foreground">
            <Server className="size-5" />
          </span>
          <p className="font-semibold">
            <Trans>No stream servers yet</Trans>
          </p>
          <p className="max-w-sm text-sm text-muted-foreground">
            <Trans>
              Add the server your encoder streams to, such as BELABOX Cloud or your own MediaMTX.
              OBS plays your feed from it, and NOALBS reads its stats to pick a scene.
            </Trans>
          </p>
          <p className="max-w-sm text-sm text-muted-foreground">
            <Trans>
              Streaming from your phone straight to Twitch or Kick? Then there’s no server to read
              yet: you need one in between.
            </Trans>{" "}
            <DocsLink page="getting-started/introduction">
              <Trans>How it connects</Trans>
            </DocsLink>
          </p>
        </div>
      )}

      {servers.data && servers.data.length > 0 && (
        <section aria-label={t`Server order`}>
          <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
            <SortableContext
              items={servers.data.map((server) => server.id)}
              strategy={verticalListSortingStrategy}
            >
              <ol>
                {servers.data.map((server, index) => (
                  <ServerRow
                    key={server.id}
                    server={server}
                    position={index + 1}
                    reading={readingOf(server)}
                    selected={server.id === selected?.id}
                    inUse={server.id === status?.active_server_id}
                    canManage={canManage}
                    path={path}
                    onSelect={() => select(server.id)}
                  />
                ))}
              </ol>
            </SortableContext>
          </DndContext>
        </section>
      )}
      <p className="border-t border-divider px-5 py-3 text-sm text-muted-foreground">{t`When no server has a feed, the BRB scene of the last one in use is shown. Click a server for its stats URL, test and scene overrides.`}</p>
      <p className="px-5 pb-3 text-sm text-muted-foreground">
        {servers.data?.length === 1
          ? t`Tip: add a second server as a backup uplink so NOALBS can fail over.`
          : t`Each server can override the LIVE, LOW and BRB scenes, e.g. a different overlay for a backup uplink.`}
      </p>
      <Sheet
        open={selected != null}
        onOpenChange={(open) => {
          if (!open) setSelectedId(null);
        }}
      >
        <SheetContent side="right" className="w-full overflow-y-auto p-0 sm:max-w-xl">
          <SheetHeader className="sr-only">
            <SheetTitle>{selected?.name}</SheetTitle>
            <SheetDescription>
              <Trans>Change how NOALBS reaches this server.</Trans>
            </SheetDescription>
          </SheetHeader>
          <section aria-label={t`Server details`}>{editor}</section>
        </SheetContent>
      </Sheet>

      {canManage && (
        <AddServerDialog open={adding} onOpenChange={setAdding} path={path} onAdded={select} />
      )}
    </Section>
  );
}

function ServerRow({
  server,
  position,
  reading,
  selected,
  inUse,
  canManage,
  path,
  onSelect,
}: {
  server: StreamServer;
  position: number;
  reading: Reading;
  selected: boolean;
  inUse: boolean;
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
        "flex items-stretch border-t border-divider",
        selected && "bg-muted/40",
        isDragging && "relative z-10 border-primary shadow-2xl",
      )}
    >
      {canManage && (
        <button
          type="button"
          ref={setActivatorNodeRef}
          {...attributes}
          {...listeners}
          aria-label={t`Move ${server.name}`}
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
            <span className="min-w-0 break-words text-sm font-semibold">{server.name}</span>
          </span>
          <span className="text-xs font-normal text-muted-foreground">{info.label}</span>
        </span>
        <span
          className={cn(
            "flex items-center gap-2 text-[13px]",
            reading.tone === "live" && "text-muted-foreground",
            reading.tone === "idle" && "text-muted-foreground",
            reading.tone === "off" && "text-subtle-foreground",
          )}
        >
          <span className="font-mono tabular">{reading.text}</span>
        </span>
      </button>
      {inUse && <span className="self-center pr-3 text-xs text-muted-foreground">{t`In use`}</span>}
      {canManage && (
        <span className="flex items-center pr-3">
          <Switch
            checked={server.enabled}
            onCheckedChange={(enabled) => toggle.mutate(enabled)}
            disabled={toggle.isPending}
            aria-label={t`Use ${server.name}`}
          />
        </span>
      )}
      <button
        type="button"
        onClick={onSelect}
        aria-label={t`Edit ${server.name}`}
        className="px-2 text-muted-foreground"
      >
        <ChevronRight className="size-4" />
      </button>
    </li>
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
  const servers = useQuery(streamServersQuery(path.workspace_id, path.switcher_id));
  const rules = useQuery(sceneRulesQuery(path.workspace_id, path.switcher_id));
  const goingRules = rulesWithServer(rules.data ?? [], server.id).map((rule) =>
    ruleLabel(rule, servers.data ?? []),
  );
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
      queryClient.invalidateQueries({
        queryKey: queryKeys.sceneRules(path.workspace_id, path.switcher_id),
      }),
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
      toast.success(t`Saved`);
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
      toast.success(t`Removed ${server.name}`);
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
          <span className="text-xs text-muted-foreground">{info.label}</span>
        </span>
        {canManage && (
          <span className="flex items-center gap-2.5 text-sm font-medium">
            {server.enabled ? t`Enabled` : t`Disabled`}
            <Switch
              checked={server.enabled}
              onCheckedChange={(enabled) => toggle.mutate(enabled)}
              disabled={toggle.isPending}
              aria-label={t`Use ${server.name}`}
            />
          </span>
        )}
      </div>

      <div className="flex flex-col gap-5 px-5 py-5 lg:px-5.5">
        <div className="flex flex-col gap-2">
          <Label htmlFor="server-editor-name">
            <Trans>Name</Trans>
          </Label>
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
            <Label htmlFor="server-editor-url">
              <Trans>Stats URL</Trans>
            </Label>
            <SecretUrlInput
              id="server-editor-url"
              value={statsUrl}
              onChange={setStatsUrl}
              label={t`stats URL`}
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
          <ConnectionTest
            server={server}
            kind={server.kind}
            statsUrl={statsUrl}
            details={details}
            path={path}
          />
        )}

        <fieldset className="flex flex-col gap-2.5">
          <legend className="text-[15px] font-semibold">
            <Trans>Scene overrides</Trans>
          </legend>
          <p className="text-[13px] text-muted-foreground">
            {obs.isSuccess
              ? t`Use different scenes while this server is the one in use.`
              : t`Connect OBS to choose scenes for this server.`}
          </p>
          <div className="grid gap-2.5 sm:grid-cols-3">
            {(["live", "low", "offline"] as const).map((role) => (
              <div key={role} className="flex flex-col gap-1.5">
                <span className="flex items-center gap-1.5 text-[13px] font-semibold">
                  <span className={cn("size-2 rounded-full", SCENE_ROLES[role].dot)} />
                  {SCENE_ROLES[role].label}
                </span>
                <ScenePicker
                  label={t`${SCENE_ROLES[role].label} override`}
                  value={overrides[role] ?? null}
                  obs={obs.data}
                  disabled={!canManage || !obs.isSuccess}
                  noneLabel={t`Use default`}
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
                <Trans>Remove server</Trans>
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>
                  <Trans>Remove {server.name}</Trans>?
                </AlertDialogTitle>
                <AlertDialogDescription>
                  <Trans>NOALBS stops reading this server’s stats.</Trans>
                  {goingRules.length > 0 && (
                    <>
                      {" "}
                      <Plural
                        value={goingRules.length}
                        one="The server combination it is in goes too:"
                        other="The # server combinations it is in go too:"
                      />{" "}
                      {goingRules.join(", ")}.
                    </>
                  )}
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>
                  <Trans>Cancel</Trans>
                </AlertDialogCancel>
                <AlertDialogAction variant="destructive" onClick={() => remove.mutate()}>
                  <Trans>Remove</Trans>
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
          <Button
            type="submit"
            disabled={!dirty || !name.trim() || !statsUrl.trim() || update.isPending}
          >
            {update.isPending ? t`Saving…` : t`Save server`}
          </Button>
        </div>
      )}
    </form>
  );
}

/**
 * Fetches the stats URL through the API and shows what came back. Runs on its
 * own for enabled servers; "Test" checks the URL as typed, before saving. Without
 * a `server` it tests one that isn't added yet.
 */
function ConnectionTest({
  server,
  kind,
  statsUrl,
  details,
  path,
}: {
  server?: StreamServer;
  kind: StreamServerKind;
  statsUrl: string;
  /** The kind-specific settings as typed; stored secrets fill in the rest. */
  details: DetailsForm;
  path: Path;
}) {
  const [tested, setTested] = useState({
    url: server?.stats_url ?? "",
    details: detailsInput(details),
    asked: server?.enabled ?? false,
  });
  const probe = useQuery({
    queryKey: ["stats-probe", server?.id ?? "new", kind, tested.url, tested.details],
    queryFn: () =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers/test", {
          params: { path },
          body: {
            kind,
            stats_url: tested.url,
            details: tested.details,
            stream_server_id: server?.id,
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

  let title = t`Not checked`;
  let detail =
    server?.enabled === false ? t`Server is disabled` : t`Checks the stats URL from NOALBS Cloud.`;
  if (checking) {
    title = t`Checking…`;
    detail = maskUrl(tested.url);
  } else if (probe.isError) {
    title = t`Couldn’t test`;
    detail = errorMessage(probe.error);
  } else if (result) {
    title = result.reachable ? (result.receiving ? t`Receiving` : t`Reachable`) : t`Not reachable`;
    const facts = [
      result.http_status != null && `HTTP ${result.http_status}`,
      result.elapsed_ms != null && t`in ${result.elapsed_ms} ms`,
      result.bitrate != null && t`bitrate ${formatKbps(result.bitrate)} kbps`,
      result.rtt != null && t`rtt ${Math.round(result.rtt)} ms`,
    ].filter(Boolean);
    detail = [facts.join(" · "), result.message].filter(Boolean).join(" · ");
  }

  return (
    <div
      role="status"
      aria-label={t`Connection test`}
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
        {result || probe.isError ? t`Test again` : t`Test`}
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
            <DialogTitle>
              <Trans>Add a stream server</Trans>
            </DialogTitle>
            <DialogDescription>
              <Trans>
                Pick the server your encoder streams to and OBS plays from. Not sure which one you
                have? See the{" "}
                <DocsLink page="reference/stream-server-kinds">list of servers</DocsLink>.
              </Trans>
            </DialogDescription>
          </DialogHeader>

          <fieldset className="grid grid-cols-2 gap-2 sm:grid-cols-3">
            <legend className="sr-only">
              <Trans>Server type</Trans>
            </legend>
            {streamServerKinds().map((option) => (
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
            <Label htmlFor="server-name">
              <Trans>Name</Trans>
            </Label>
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
            <Label htmlFor="server-stats-url">
              <Trans>Stats URL</Trans>
            </Label>
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

          <ConnectionTest kind={kind} statsUrl={statsUrl} details={details} path={path} />

          {create.error && <p className="text-sm text-destructive">{errorMessage(create.error)}</p>}

          <DialogFooter>
            <Button type="submit" disabled={!name.trim() || !statsUrl.trim() || create.isPending}>
              {create.isPending ? t`Adding…` : t`Add server`}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
