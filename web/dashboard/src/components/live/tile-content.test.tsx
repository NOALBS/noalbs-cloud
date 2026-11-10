import { i18n } from "@lingui/core";
import { I18nProvider } from "@lingui/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { obsScenesQuery, obsSourcesQuery } from "@/api/queries";
import type { BelaboxStatus, ObsScenes, StreamSummary } from "@/api/types";
import { BelaboxSummary, MoblinSummary } from "./device-summary";
import { Markers, runningStream } from "./markers";
import { SceneGrid, sceneGroups } from "./scene-grid";
import { SourceList } from "./source-list";
import { LiveTile } from "./tile";

const path = { workspace_id: "w", switcher_id: "s" };
let root: Root;
let host: HTMLDivElement;
let client: QueryClient;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  client.clear();
  localStorage.clear();
  vi.unstubAllGlobals();
});
const markup = (node: ReactNode) =>
  renderToStaticMarkup(<I18nProvider i18n={i18n}>{node}</I18nProvider>);
function render(node: ReactNode) {
  act(() =>
    root.render(
      <I18nProvider i18n={i18n}>
        <QueryClientProvider client={client}>{node}</QueryClientProvider>
      </I18nProvider>,
    ),
  );
}
const scenes: ObsScenes["scenes"] = Array.from({ length: 25 }, (_, i) => ({
  uuid: `scene-${i}`,
  name: `Scene ${i}`,
}));

it("groups large scene lists by role in OBS order and filters without changing the small grid", () => {
  const roles = { live: "scene-4", offline: "scene-1" };
  expect(sceneGroups(scenes, roles, "")[0]?.items.map((scene) => scene.uuid)).toEqual([
    "scene-1",
    "scene-4",
  ]);
  expect(sceneGroups(scenes, roles, "SCENE 2")[1]?.items.map((scene) => scene.uuid)).toEqual([
    "scene-2",
    "scene-20",
    "scene-21",
    "scene-22",
    "scene-23",
    "scene-24",
  ]);
  expect(sceneGroups(scenes, roles, "missing").flatMap((group) => group.items)).toEqual([]);
  expect(sceneGroups([], roles, "")).toEqual([{ withRole: null, items: [] }]);
  expect(sceneGroups(scenes.slice(0, 3), roles, "missing")[0]?.items).toHaveLength(3);
});

it("puts scene controls in the tile header, keeps customize/manage, and distinguishes on air from in OBS", () => {
  client.setQueryData(obsScenesQuery("w", "s").queryKey, {
    scenes: scenes.slice(0, 3),
    current_scene_uuid: "scene-1",
    current_scene_name: "Scene 1",
  });
  const grid = (streaming: boolean) => (
    <LiveTile title="Scenes">
      <SceneGrid
        path={path}
        scenes={{ live: "scene-1" }}
        currentUuid="scene-1"
        canControl
        canManage
        obsConnected
        streaming={streaming}
        feedState="low"
      />
    </LiveTile>
  );
  render(grid(true));
  expect(host.querySelectorAll("h2")).toHaveLength(1);
  expect(host.querySelector('[aria-pressed="true"]')?.textContent).toContain("On air");
  expect(host.querySelector('[aria-pressed="true"] .bg-low')).not.toBeNull();
  expect(host.querySelector('header button[aria-label="Customize"]')).not.toBeNull();
  act(() => host.querySelector<HTMLButtonElement>('button[aria-label="Customize"]')?.click());
  expect(host.textContent).toContain("Only this browser remembers it");
  act(() =>
    Array.from(host.querySelectorAll("button"))
      .find((button) => button.textContent === "Done")
      ?.click(),
  );
  render(grid(false));
  expect(host.querySelector('[aria-pressed="true"]')?.textContent).toContain("In OBS");
  act(() => host.querySelector<HTMLButtonElement>('button[aria-label="Manage scenes"]')?.click());
  expect(host.textContent).toContain("made in OBS");
});

it("uses two source columns without preview, retains controls and renders empty/offline messages without a second heading", () => {
  client.setQueryData(obsSourcesQuery("w", "s", "scene").queryKey, { scene: "Scene", sources: [] });
  render(
    <LiveTile title="Sources">
      <SourceList path={path} scene="scene" canControl canEditSources obsConnected columns />
    </LiveTile>,
  );
  expect(host.querySelectorAll("h2")).toHaveLength(1);
  expect(host.textContent).toContain("no sources");
  render(
    <LiveTile title="Sources">
      <SourceList
        path={path}
        scene="scene"
        canControl={false}
        canEditSources={false}
        obsConnected={false}
        columns
      />
    </LiveTile>,
  );
  expect(host.textContent).toContain("once OBS is connected");
});

const stream = (id: string, started_at: string, ended_at: string | null): StreamSummary => ({
  id,
  started_at,
  ended_at,
  duration_seconds: 1,
  live_seconds: 1,
  low_seconds: 0,
  offline_seconds: 0,
  auto_switches: 0,
});
it("selects the newest unfinished stream only while streaming, and renders marker normal/empty/error states", () => {
  const list = [
    stream("old", "2026-01-01", null),
    stream("ended", "2026-01-03", "2026-01-04"),
    stream("current", "2026-01-02", null),
  ];
  expect(runningStream(list, true)?.id).toBe("current");
  expect(runningStream(list, false)).toBeUndefined();
  expect(runningStream([], true)).toBeUndefined();
  expect(markup(<Markers markers={[]} error={false} loading={false} />)).toContain(
    "No markers yet",
  );
  expect(markup(<Markers markers={undefined} error loading={false} />)).toContain(
    "Couldn’t load markers",
  );
  expect(markup(<Markers markers={undefined} error={false} loading />)).toContain("Loading…");
  expect(
    markup(
      <Markers
        markers={[{ id: "marker", at: "2026-01-02", offset_seconds: 62, description: "A moment" }]}
        error={false}
        loading={false}
      />,
    ),
  ).toContain("1:02");
});

const now = Date.parse("2026-01-01T12:00:00Z");
const belabox: BelaboxStatus = {
  updated_at: new Date(now).toISOString(),
  link: "online",
  streaming: true,
  total_kbps: 4560,
  max_br: 6000,
  interfaces: [{ name: "usb0", kbps: 4560, enabled: true }],
  sensors: [],
  notifications: [],
};
it("shows device summaries and their disconnected/error/paused help with full controls inside a right sheet", () => {
  render(
    <BelaboxSummary
      status={belabox}
      now={now}
      labels={{}}
      paused
      details={<button type="button">Full controls</button>}
    />,
  );
  expect(host.textContent).toContain("4,560");
  expect(host.textContent).toContain("Controls are off");
  act(() => host.querySelector<HTMLButtonElement>("button")?.click());
  expect(document.querySelector('[role="dialog"]')?.textContent).toContain("Full controls");
  expect(document.querySelector('[data-slot="sheet-content"]')?.className).toContain("right-0");
  expect(
    markup(<BelaboxSummary status={null} now={now} labels={{}} paused={false} details={null} />),
  ).toContain("Not connected");
  expect(
    markup(
      <BelaboxSummary
        status={{ ...belabox, link: "invalid_key" }}
        now={now}
        labels={{}}
        paused={false}
        details={null}
      />,
    ),
  ).toContain("refused the secret URL");
  expect(markup(<MoblinSummary status={null} now={now} paused={false} details={null} />)).toContain(
    "Remote control",
  );
});
