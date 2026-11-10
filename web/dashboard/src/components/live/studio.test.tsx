import { i18n } from "@lingui/core";
import { I18nProvider } from "@lingui/react";
import type { ComponentProps, ReactNode } from "react";
import { renderToStaticMarkup as renderMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { StreamServer, SwitcherStatus } from "@/api/types";
import { initialLiveState, type Point } from "@/live/reducer";
import { BitrateChart } from "./bitrate-chart";
import { LiveHero, lastFreshAt } from "./hero";
import { ServerCard } from "./server-card";
import { LiveTile } from "./tile";

const renderToStaticMarkup = (node: ReactNode) =>
  renderMarkup(<I18nProvider i18n={i18n}>{node}</I18nProvider>);

const now = Date.parse("2026-09-24T12:00:00Z");
const status: SwitcherStatus = {
  obs_connected: true,
  streaming: true,
  state: "live",
  auto_switch: true,
  browser_source: true,
  recording: false,
  recording_paused: false,
  updated_at: new Date(now).toISOString(),
  active_server_id: "a",
  current_scene: { uuid: "scene", name: "IRL" },
};
const server: StreamServer = {
  id: "a",
  name: "Primary",
  kind: "belabox",
  enabled: true,
  priority: 0,
  overrides: {},
};
const points: Point[] = [
  { at: now - 1000, bitrate: 6000, rtt: 200, state: "live" },
  { at: now, bitrate: 6100, rtt: 210, state: "live" },
];
const props: ComponentProps<typeof LiveHero> = {
  state: "live",
  status,
  servers: [server],
  activeServer: server,
  live: { ...initialLiveState, history: { a: points } },
  now,
  low: 1000,
  offline: 300,
  scenesSet: true,
  narrow: false,
  paused: false,
};
const hero = (patch: Partial<typeof props> = {}) =>
  renderToStaticMarkup(<LiveHero {...props} {...patch} />);
const dom = (html: string) => {
  const node = document.createElement("div");
  node.innerHTML = html;
  return node;
};

describe("studio hero", () => {
  it("shows real headline, scene and all server series", () => {
    const node = dom(
      hero({
        servers: [server, { ...server, id: "b", name: "Backup" }],
        live: { ...props.live, history: { a: points, b: points } },
      }),
    );
    expect(node.textContent).toContain("6,100");
    expect(node.textContent).toContain("210");
    expect(node.textContent).toContain("IRL");
    expect(node.textContent).toContain("Backup");
    expect(node.querySelectorAll("polyline")).toHaveLength(2);
    expect(node.querySelectorAll("polygon")).toHaveLength(1);
    expect(node.querySelectorAll("line")).toHaveLength(2);
  });
  it("handles connecting, offline and no samples without inventing numbers", () => {
    expect(hero({ state: "connecting", status: undefined })).toContain("Connecting…");
    expect(hero({ state: "browserOffline", status: null })).toContain("OBS is offline");
    expect(
      hero({ state: "obsUnreachable", status: { ...status, obs_connected: false } }),
    ).toContain("OBS is offline");
    const empty = dom(hero({ live: initialLiveState, activeServer: undefined, servers: [] }));
    expect(empty.textContent).toContain("—");
    expect(empty.querySelectorAll("polyline")).toHaveLength(0);
  });
  it("shows not-streaming checks, paused and low chips", () => {
    const node = dom(hero({ state: "notStreaming", status: { ...status, streaming: false } }));
    expect(node.textContent).toContain("No feed yet");
    expect(node.textContent).toContain("In OBS");
    expect(node.querySelectorAll('li [aria-label="Done"]')).toHaveLength(3);
    expect(hero({ state: "paused", paused: true })).toContain("Not switching while paused");
    expect(hero({ state: "low", status: { ...status, state: "low" } })).toContain("LOW");
  });
  it("shows BRB duration only when fresh history exists, never a fabricated stop countdown", () => {
    const offline = { ...status, state: "offline" as const };
    const html = hero({ state: "noFeed", status: offline, now: now + 42000 });
    expect(html).toContain("BRB");
    expect(dom(html).textContent).toContain("for 0:42");
    expect(html).not.toContain("stream stops");
    const unknown = dom(hero({ state: "noFeed", status: offline, live: initialLiveState }));
    expect(unknown.textContent).toContain("No feed");
    expect(unknown.querySelector("h3")?.textContent).not.toContain("for");
    expect(lastFreshAt({})).toBeNull();
    expect(lastFreshAt({ a: [{ ...(points[0] as Point), state: "offline" }] })).toBeNull();
  });
});

it("keeps the single-series chart API and handles empty/missing series", () => {
  expect(
    dom(
      renderToStaticMarkup(
        <BitrateChart points={points} now={now} low={null} offline={null} tone="text-live" />,
      ),
    ).querySelectorAll("polygon"),
  ).toHaveLength(1);
  expect(
    dom(
      renderToStaticMarkup(
        <BitrateChart series={[]} now={now} low={null} offline={null} tone="text-live" />,
      ),
    ).querySelectorAll("polyline"),
  ).toHaveLength(0);
});

it("uses row-based server data, fresh/offline/disabled states and no kind chips", () => {
  const render = (input: Partial<ComponentProps<typeof ServerCard>> = {}) =>
    dom(
      renderToStaticMarkup(
        <ServerCard server={server} points={points} now={now} inUse {...input} />,
      ),
    );
  expect(render().textContent).toContain("in use");
  expect(render().textContent).toContain("6,100");
  expect(render({ now: now + 10000 }).textContent).toContain("No feed");
  expect(render({ server: { ...server, enabled: false } }).textContent).toContain("Disabled");
  expect(render({ points: [] }).textContent).toContain("No feed");
});

it("dims OBS-dependent tile chrome but keeps its right-side controls reachable", () => {
  const node = dom(
    renderToStaticMarkup(
      <LiveTile title="Actions" right={<button type="button">Connect</button>} needsObs>
        <p>Body</p>
      </LiveTile>,
    ),
  );
  expect(node.querySelector("section")?.className).toContain("opacity-50");
  expect(node.textContent).toContain("Needs OBS");
  expect(node.querySelector("button")?.textContent).toBe("Connect");
});
