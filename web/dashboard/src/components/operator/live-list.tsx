import { plural, t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { ChevronRight, Search } from "lucide-react";
import { createContext, useContext, useEffect, useState } from "react";
import { Pager, SearchBox } from "@/components/admin/list-controls";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useMediaQuery } from "@/hooks/use-media-query";
import { useNow } from "@/hooks/use-now";
import { formatKbps, formatUptime } from "@/lib/live-format";
import type { LiveSearch } from "@/lib/organizations";
import { roleLabels } from "@/lib/permissions";
import { cn } from "@/lib/utils";
import { OperatorColumns } from "./columns";
import { CompactLive } from "./compact-live";
import { offlineSummary, problemLine, stateChip, streamingMetrics, timeAgo } from "./format";
import { type BitrateHistory, useBitrateHistory } from "./history";
import {
  canUseRowActions,
  type Group,
  groupItems,
  type OperatorCounts,
  type OperatorData,
  type OperatorItem,
} from "./model";
import { OperatorName } from "./operator-name";
import { PhoneRow } from "./phone-row";
import { type OperatorPin, pinOf, useOperatorPins } from "./pins";
import { RowActions } from "./row-actions";
import { Sparkline } from "./sparkline";

type View = "rows" | "tiles" | "side-by-side";
const VIEW_KEY = "noalbs-operator-view";
const EMPTY_ITEMS: OperatorItem[] = [];
const Selection = createContext<{ selected?: string; onOpen?: (pin?: OperatorPin) => void }>({});

function savedView(): View | null {
  try {
    const value = localStorage.getItem(VIEW_KEY);
    return value === "rows" || value === "tiles" || value === "side-by-side" ? value : null;
  } catch {
    return null;
  }
}

function summary(counts: OperatorCounts): string {
  const { attention, connected, recording } = counts;
  if (counts.workspaces != null)
    return plural(counts.workspaces, {
      one: `${attention} need attention · ${connected} connected · ${recording} recording · in # workspace`,
      other: `${attention} need attention · ${connected} connected · ${recording} recording · in # workspaces`,
    });
  if (counts.clients != null)
    return plural(counts.clients, {
      one: `${attention} need attention · ${connected} connected · ${recording} recording · in # client`,
      other: `${attention} need attention · ${connected} connected · ${recording} recording · in # clients`,
    });
  return t`${attention} need attention · ${connected} connected · ${recording} recording`;
}

export interface LiveListProps {
  data?: OperatorData;
  updatedAt: number;
  pending: boolean;
  error?: string;
  scope: "workspaces" | "organization" | "workspace";
  search: LiveSearch;
  onSearch: (q: string) => void;
  onShow: (show: LiveSearch["show"]) => void;
  onPage?: (page: number) => void;
  onOpen: (pin?: OperatorPin) => void;
}

export function LiveList({
  data,
  updatedAt,
  pending,
  error,
  scope,
  search,
  onSearch,
  onShow,
  onPage,
  onOpen,
}: LiveListProps) {
  const now = useNow();
  const desktop = useMediaQuery("(min-width: 64rem)");
  const phone = useMediaQuery("(max-width: 47.999rem)");
  const [searchExpanded, setSearchExpanded] = useState(Boolean(search.q));
  const { pins, save } = useOperatorPins();
  const wide = useMediaQuery("(min-width: 1921px)");
  const [choice, setChoice] = useState(savedView);
  const view = phone ? "rows" : (choice ?? (wide ? "tiles" : "rows"));
  const [showOffline, setShowOffline] = useState(false);
  const items = data?.items ?? EMPTY_ITEMS;
  const openWorkspace =
    search.open_workspace ?? items.find((item) => item.switcher_id === search.open)?.workspace_slug;
  const panel =
    desktop && view !== "side-by-side" && search.open && openWorkspace
      ? { workspace: openWorkspace, switcher: search.open }
      : null;
  const panelOpen = Boolean(panel);
  useEffect(() => {
    if (!panelOpen) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.defaultPrevented) onOpen(undefined);
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [panelOpen, onOpen]);
  const history = useBitrateHistory(items, updatedAt);
  const groups = groupItems(items, scope === "workspace" ? search : undefined);
  const nothingConnected = data?.counts.connected === 0;
  const offlineExpanded = nothingConnected || showOffline;
  const choose = (value: View) => {
    setChoice(value);
    try {
      localStorage.setItem(VIEW_KEY, value);
    } catch {
      /* Storage may be unavailable. */
    }
  };
  const filters: { value: LiveSearch["show"]; label: string }[] = [
    { value: undefined, label: t`All connected` },
    { value: "live", label: t`Streaming` },
    { value: "attention", label: t`Needs attention` },
  ];
  const views: { value: View; label: string }[] = [
    { value: "rows", label: t`Rows` },
    { value: "tiles", label: t`Tiles` },
    { value: "side-by-side", label: t`Side by side` },
  ];
  const connectedShown = groups.attention.length + groups.streaming.length + groups.idle.length;
  const seen = groups.offline.filter((item) => item.last_seen_at);
  const recentSeen = seen[0]?.last_seen_at;
  const oldestSeen = seen[seen.length - 1]?.last_seen_at;
  const offlineRange =
    recentSeen && oldestSeen
      ? t`Last seen ${timeAgo(recentSeen, now)} to ${timeAgo(oldestSeen, now)}`
      : t`Never connected`;

  return (
    <Selection.Provider value={{ selected: panel?.switcher, onOpen: desktop ? onOpen : undefined }}>
      <section
        aria-label={t`Live switchers`}
        className="@container flex w-full min-w-0 flex-col gap-4"
      >
        {phone ? (
          <div className="flex flex-col gap-3">
            <div className="flex items-center justify-between gap-3">
              <h2 className="font-display text-xl font-extrabold">
                {nothingConnected ? t`Nothing is connected` : t`Switchers`}
                {!nothingConnected && data && (
                  <span className="ml-2 text-sm font-normal text-muted-foreground">
                    {data.counts.connected}
                  </span>
                )}
              </h2>
              <Button
                size="icon-sm"
                variant="ghost"
                aria-label={t`Find a switcher`}
                aria-expanded={searchExpanded}
                aria-controls="operator-search"
                onClick={() => setSearchExpanded(!searchExpanded)}
              >
                <Search className="size-4" />
              </Button>
            </div>
            {nothingConnected && data && (
              <p className="text-xs text-muted-foreground">
                {offlineSummary(data.offlineTotal, data.counts)}
              </p>
            )}
            {searchExpanded && (
              <div id="operator-search">
                <SearchBox
                  value={search.q ?? ""}
                  onSearch={onSearch}
                  placeholder={t`Find a switcher`}
                />
              </div>
            )}
            <fieldset aria-label={t`Show`} className="grid grid-cols-3 rounded-lg bg-muted p-0.5">
              {[
                { value: undefined, label: t`All ${data?.counts.connected ?? 0}` },
                { value: "live" as const, label: t`Streaming ${data?.counts.streaming ?? 0}` },
                { value: "attention" as const, label: t`Attention ${data?.counts.attention ?? 0}` },
              ].map((filter) => (
                <Button
                  key={filter.label}
                  size="sm"
                  variant={search.show === filter.value ? "secondary" : "ghost"}
                  aria-pressed={search.show === filter.value}
                  onClick={() => onShow(filter.value)}
                  className="px-1 text-xs"
                >
                  {filter.label}
                </Button>
              ))}
            </fieldset>
          </div>
        ) : (
          <div className="flex flex-wrap items-center justify-between gap-4 xl:flex-nowrap">
            <div className="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-4 gap-y-1">
              <h2 className="whitespace-nowrap font-display text-2xl font-extrabold tracking-tight">
                {nothingConnected
                  ? t`Nothing is connected`
                  : data
                    ? plural(data.counts.streaming, { one: "# streaming", other: "# streaming" })
                    : t`Live switchers`}
              </h2>
              {data && (
                <p className="text-xs text-muted-foreground">
                  {nothingConnected
                    ? offlineSummary(data.offlineTotal, data.counts)
                    : summary(data.counts)}
                </p>
              )}
            </div>
            <div className="flex w-full flex-wrap items-center gap-2 xl:w-auto xl:shrink-0 xl:flex-nowrap">
              <div className="w-full sm:w-48 xl:min-w-44">
                <SearchBox
                  value={search.q ?? ""}
                  onSearch={onSearch}
                  placeholder={t`Find a switcher`}
                />
              </div>
              <fieldset
                aria-label={t`Show`}
                className="flex max-w-full flex-wrap rounded-lg bg-muted p-0.5"
              >
                {filters.map((filter) => (
                  <Button
                    key={filter.label}
                    size="sm"
                    variant={search.show === filter.value ? "secondary" : "ghost"}
                    aria-pressed={search.show === filter.value}
                    onClick={() => onShow(filter.value)}
                    className="text-xs"
                  >
                    {filter.label}
                  </Button>
                ))}
                {search.show === "obs" && (
                  <Button
                    size="sm"
                    variant="secondary"
                    aria-pressed
                    onClick={() => onShow(undefined)}
                  >
                    <Trans>OBS connected</Trans>
                  </Button>
                )}
              </fieldset>
              <fieldset aria-label={t`List view`} className="flex rounded-lg bg-muted p-0.5">
                {views.map((option) => (
                  <Button
                    key={option.value}
                    size="sm"
                    variant={view === option.value ? "secondary" : "ghost"}
                    aria-pressed={view === option.value}
                    onClick={() => choose(option.value)}
                    className="text-xs"
                  >
                    {option.label}
                  </Button>
                ))}
              </fieldset>
            </div>
          </div>
        )}
        {nothingConnected && (
          <p className="text-sm text-muted-foreground">
            <Trans>
              Switchers show up here when their OBS is open with the browser source active.
            </Trans>
          </p>
        )}
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        {view === "side-by-side" ? (
          <OperatorColumns
            items={items}
            pins={pins}
            onChange={save}
            singleWorkspace={scope === "workspace"}
          />
        ) : (
          <div
            className={cn(
              "grid min-w-0 items-start gap-4",
              panel && "lg:grid-cols-[minmax(0,1fr)_minmax(380px,440px)]",
            )}
          >
            <div className="@container flex min-w-0 flex-col gap-4">
              {pending && !data ? (
                <Skeleton className="h-48 rounded-xl" />
              ) : (
                <>
                  {(["attention", "streaming", "idle"] as const).map(
                    (group) =>
                      groups[group].length > 0 && (
                        <LiveGroup
                          key={group}
                          group={group}
                          items={groups[group]}
                          multi={scope !== "workspace"}
                          view={view === "tiles" ? "tiles" : "rows"}
                          now={now}
                          history={history}
                          phone={phone}
                        />
                      ),
                  )}
                  {!nothingConnected && data && connectedShown === 0 && (
                    <p className="py-8 text-center text-sm text-muted-foreground">
                      <Trans>Nothing matches.</Trans>
                    </p>
                  )}
                  {data && onPage && data.total > data.perPage && (
                    <Pager
                      page={search.page ?? 1}
                      total={data.total}
                      perPage={data.perPage}
                      onPage={onPage}
                    />
                  )}
                  {groups.offline.length > 0 && (
                    <>
                      {!nothingConnected && (
                        <div className="flex flex-wrap items-center justify-between gap-2 px-3 text-xs text-muted-foreground">
                          <span>
                            {plural(data?.offlineTotal ?? groups.offline.length, {
                              one: "# more switcher is offline",
                              other: "# more switchers are offline",
                            })}
                            <span className="ml-3">{offlineRange}</span>
                          </span>
                          <Button
                            variant="ghost"
                            size="sm"
                            aria-expanded={offlineExpanded}
                            aria-controls="operator-offline"
                            onClick={() => setShowOffline(!showOffline)}
                            className="text-primary"
                          >
                            {offlineExpanded ? t`Hide` : t`Show`}
                          </Button>
                        </div>
                      )}
                      {offlineExpanded && (
                        <section
                          id="operator-offline"
                          className="overflow-hidden rounded-xl border bg-card shadow-tile"
                        >
                          <GroupHeading
                            title={t`Offline`}
                            count={data?.offlineTotal ?? groups.offline.length}
                            hint={t`Most recent first`}
                          />
                          <ul>
                            {groups.offline.map((item) => (
                              <OfflineRow
                                key={item.switcher_id}
                                item={item}
                                multi={scope !== "workspace"}
                                now={now}
                              />
                            ))}
                          </ul>
                          {data && data.offlineTotal > groups.offline.length && (
                            <p className="border-t border-divider px-4 py-3 text-xs text-muted-foreground">
                              <Trans>
                                Showing the {groups.offline.length} most recently seen switchers.
                              </Trans>
                            </p>
                          )}
                        </section>
                      )}
                    </>
                  )}
                </>
              )}
            </div>
            {panel && (
              <aside
                aria-label={t`Live panel`}
                className="hidden min-w-0 lg:sticky lg:top-20 lg:block lg:max-h-[calc(100vh-6rem)] lg:overflow-y-auto"
              >
                <CompactLive
                  singleWorkspace={scope === "workspace"}
                  pin={panel}
                  pinned={pins.some((pin) => pin.switcher === panel.switcher)}
                  canPin={pins.length < 4}
                  onPin={() =>
                    save(
                      pins.some((pin) => pin.switcher === panel.switcher)
                        ? pins.filter((pin) => pin.switcher !== panel.switcher)
                        : [...pins, panel],
                    )
                  }
                  onClose={() => onOpen(undefined)}
                />
              </aside>
            )}
          </div>
        )}
      </section>
    </Selection.Provider>
  );
}

function GroupHeading({
  title,
  count,
  hint,
  phone,
}: {
  title: string;
  count: number;
  hint?: string;
  phone?: boolean;
}) {
  return (
    <header
      className={cn(
        "flex flex-wrap items-center justify-between gap-2 py-2.5 text-xs",
        phone ? "px-1 text-muted-foreground" : "border-b border-divider px-4",
      )}
    >
      <h3 className="font-semibold">
        {title}
        <span className="ml-2 font-normal text-muted-foreground">{count}</span>
      </h3>
      {hint && <p className="text-muted-foreground">{hint}</p>}
    </header>
  );
}

function LiveGroup({
  group,
  items,
  view,
  multi,
  now,
  history,
  phone,
}: {
  group: Exclude<Group, "offline">;
  items: OperatorItem[];
  view: "rows" | "tiles";
  multi: boolean;
  now: number;
  history: BitrateHistory;
  phone: boolean;
}) {
  const title =
    group === "attention"
      ? t`Needs attention`
      : group === "streaming"
        ? t`Streaming`
        : t`Not streaming`;
  const hint =
    group === "attention"
      ? t`Longest first`
      : group === "streaming"
        ? t`Bitrate and RTT of the server in use · last 2 min`
        : undefined;
  return (
    <section
      aria-label={title}
      className={cn(
        !phone && view === "rows" && "overflow-hidden rounded-xl border bg-card shadow-tile",
      )}
    >
      <GroupHeading
        title={title}
        count={items.length}
        hint={phone ? undefined : hint}
        phone={phone}
      />
      <ul
        className={cn(
          phone && "overflow-hidden rounded-xl border bg-card",
          view === "tiles" &&
            "mt-3 grid grid-cols-[repeat(auto-fill,minmax(min(100%,230px),1fr))] items-start gap-3",
        )}
      >
        {items.map((item) =>
          phone ? (
            <PhoneRow key={item.switcher_id} item={item} multi={multi} now={now} />
          ) : (
            <LiveItem
              key={item.switcher_id}
              item={item}
              tile={view === "tiles"}
              multi={multi}
              now={now}
              history={history}
            />
          ),
        )}
      </ul>
    </section>
  );
}

function ItemName({ item, multi }: { item: OperatorItem; multi: boolean }) {
  const selection = useContext(Selection);
  return (
    <Link
      to="/$workspace/s/$switcher/live"
      params={{ workspace: item.workspace_slug, switcher: item.switcher_id }}
      aria-current={selection.selected === item.switcher_id ? "true" : undefined}
      onClick={(event) => {
        if (
          selection.onOpen &&
          !event.metaKey &&
          !event.ctrlKey &&
          !event.shiftKey &&
          !event.altKey &&
          event.button === 0
        ) {
          event.preventDefault();
          selection.onOpen(pinOf(item));
        }
      }}
      className="font-semibold after:absolute after:inset-0 focus-visible:outline-none focus-visible:after:ring-2 focus-visible:after:ring-ring focus-visible:after:ring-inset"
    >
      <OperatorName workspace={item.workspace_name} switcher={item.switcher_name} multi={multi} />
    </Link>
  );
}

function LiveItem({
  item,
  tile,
  multi,
  now,
  history,
}: {
  item: OperatorItem;
  tile: boolean;
  multi: boolean;
  now: number;
  history: BitrateHistory;
}) {
  const chip = stateChip(item);
  const selected = useContext(Selection).selected === item.switcher_id;
  const problem = problemLine(item, now);
  const role = item.role ? roleLabels()[item.role] : null;
  const actions = canUseRowActions(item);
  const { bitrate, rtt: serverRtt } = streamingMetrics(item);
  const points =
    item.streaming && !item.paused && item.obs_connected ? history[item.switcher_id] : [];
  const colour =
    item.streaming && item.state === "live"
      ? "text-live"
      : item.state === "low"
        ? "text-low"
        : item.state === "offline"
          ? "text-offline"
          : "text-muted-foreground";
  const status = (
    <span
      className={cn(
        "inline-flex w-fit whitespace-nowrap rounded px-1.5 py-0.5 text-[10px] font-semibold",
        chip.className,
      )}
    >
      {chip.label}
    </span>
  );
  const uptime = (
    <span className="whitespace-nowrap text-xs text-muted-foreground">
      {item.streaming && item.stream_started_at ? formatUptime(item.stream_started_at, now) : "—"}
    </span>
  );
  const rec = item.recording && (
    <span className="inline-flex items-center gap-1 text-[10px] text-muted-foreground">
      <span className="size-1.5 rounded-full bg-offline" />
      <Trans>REC</Trans>
    </span>
  );
  const numbers = (
    <span className="whitespace-nowrap">
      <span className={cn("font-display font-bold tabular-nums", tile ? "text-2xl" : "text-sm")}>
        {bitrate != null ? formatKbps(bitrate) : "—"}
      </span>
      {bitrate != null && (
        <span className="ml-1 text-[10px] text-muted-foreground">
          <Trans>kbps</Trans>
        </span>
      )}
    </span>
  );
  const rtt = (
    <span className="whitespace-nowrap text-[10px] text-muted-foreground">
      {serverRtt != null ? t`${Math.round(serverRtt)} ms` : "—"}
    </span>
  );
  const actionClass = cn(
    "z-10",
    !item.attention &&
      "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100 has-[[data-state=open]]:opacity-100",
  );
  if (tile)
    return (
      <li
        className={cn(
          "group relative flex min-w-0 flex-col gap-2 rounded-xl border bg-card p-3 shadow-tile",
          selected && "border-primary bg-primary-soft ring-1 ring-primary",
          item.attention === "no_feed"
            ? "border-offline/30"
            : item.attention
              ? "border-low/30"
              : "border-divider",
        )}
      >
        <div className="flex flex-wrap items-center gap-2">
          {status}
          {item.streaming && uptime}
          <span className="ml-auto">{rec}</span>
        </div>
        <div className="min-w-0 text-sm">
          <ItemName item={item} multi={multi} />
        </div>
        {(item.streaming || item.attention) && (
          <>
            <div className="flex items-baseline justify-between gap-2">
              {numbers}
              {rtt}
            </div>
            <Sparkline
              points={points}
              now={now}
              className={cn("w-full shrink-0", item.streaming ? "h-10" : "h-3", colour)}
            />
          </>
        )}
        {problem && <p className="text-xs text-muted-foreground">{problem}</p>}
        {!problem && !item.streaming && (
          <p className="text-xs text-muted-foreground">
            {item.paused
              ? t`Over the plan’s switcher limit`
              : item.scene
                ? t`In OBS: ${item.scene}`
                : t`No scene`}
          </p>
        )}
        {item.streaming && (
          <p className="mt-auto border-t border-divider pt-2 text-xs text-muted-foreground">
            {item.scene ?? t`Unknown scene`}
            {item.server?.name ? t` · ${item.server.name}` : ""}
          </p>
        )}
        {actions && (
          <div className={actionClass}>
            <RowActions item={item} />
          </div>
        )}
      </li>
    );
  return (
    <li
      className={cn(
        "group relative grid grid-cols-[78px_minmax(0,2fr)_minmax(0,1fr)_58px_58px_30px_12px] items-center gap-x-2 gap-y-2 border-b border-divider border-l-2 border-l-transparent px-3 py-3 last:border-b-0 hover:bg-surface-2/30 @min-[1100px]:grid-cols-[78px_minmax(140px,2.5fr)_minmax(90px,1.3fr)_minmax(60px,.8fr)_54px_minmax(70px,1.1fr)_76px_30px_174px_12px] @min-[1100px]:gap-x-3",
        selected && "border-l-primary bg-primary-soft",
      )}
    >
      <div>{status}</div>
      <div className="min-w-0 truncate text-sm">
        <ItemName item={item} multi={multi} />
        {(problem || role || item.paused) && (
          <p className="mt-0.5 truncate text-xs text-muted-foreground">
            {problem ?? (item.paused ? t`Over the plan’s switcher limit` : role)}
          </p>
        )}
      </div>
      <div className="min-w-0 text-xs">
        <p className="truncate font-medium">{item.scene ?? t`Unknown scene`}</p>
        <p className="mt-0.5 text-[10px] text-muted-foreground">
          {item.streaming ? t`On air` : t`In OBS`}
        </p>
      </div>
      <div className="text-right">{numbers}</div>
      <div className="hidden text-right @min-[1100px]:block">{rtt}</div>
      <Sparkline
        points={points}
        now={now}
        className={cn("hidden h-7 w-full @min-[1100px]:block", colour)}
      />
      <div className="text-right">{uptime}</div>
      <div>{rec}</div>
      <div
        className={cn(
          "flex items-center justify-end @min-[1100px]:static @min-[1100px]:col-span-1 @min-[1100px]:col-start-auto @min-[1100px]:row-auto @min-[1100px]:min-h-8 @min-[1100px]:translate-y-0",
          item.attention
            ? "col-span-5 col-start-2 row-start-2 justify-start"
            : "absolute right-8 top-1/2 -translate-y-1/2 bg-card",
          actions && actionClass,
        )}
      >
        {actions && <RowActions item={item} />}
      </div>
      <ChevronRight
        className="col-start-7 row-start-1 size-3 text-muted-foreground @min-[1100px]:col-start-auto @min-[1100px]:row-start-auto"
        aria-hidden="true"
      />
    </li>
  );
}

function OfflineRow({ item, multi, now }: { item: OperatorItem; multi: boolean; now: number }) {
  return (
    <li className="relative grid grid-cols-[64px_minmax(0,1fr)_auto_12px] items-center gap-x-3 gap-y-2 border-b border-divider px-4 py-4 text-sm last:border-b-0 hover:bg-surface-2/30 @min-[1100px]:grid-cols-[78px_minmax(140px,2.5fr)_minmax(90px,1.3fr)_minmax(60px,.8fr)_54px_minmax(70px,1.1fr)_76px_30px_174px_12px]">
      <span className="text-xs text-muted-foreground">
        <Trans>Offline</Trans>
      </span>
      <div className="min-w-0">
        <ItemName item={item} multi={multi} />
        {item.role && (
          <p className="mt-0.5 text-xs text-muted-foreground">{roleLabels()[item.role]}</p>
        )}
      </div>
      <span className="col-span-2 col-start-2 row-start-2 text-xs text-muted-foreground @min-[1100px]:col-span-5 @min-[1100px]:col-start-3 @min-[1100px]:row-start-1">
        {item.last_seen_at ? t`Last seen ${timeAgo(item.last_seen_at, now)}` : t`Never connected`}
      </span>
      <div className="relative z-10 col-start-3 row-start-1 justify-self-end @min-[1100px]:col-span-2 @min-[1100px]:col-start-8">
        {!item.last_seen_at && (
          <Button asChild size="sm" variant="secondary" className="relative z-10">
            <Link
              to="/$workspace/s/$switcher/setup"
              params={{ workspace: item.workspace_slug, switcher: item.switcher_id }}
            >
              <Trans>Set up</Trans>
            </Link>
          </Button>
        )}
      </div>
      <ChevronRight
        className="col-start-4 row-start-1 size-3 text-muted-foreground @min-[1100px]:col-start-10"
        aria-hidden="true"
      />
    </li>
  );
}
