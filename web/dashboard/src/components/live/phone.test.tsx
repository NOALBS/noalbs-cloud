import { i18n } from "@lingui/core";
import { I18nProvider } from "@lingui/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { obsScenesQuery } from "@/api/queries";
import type { SwitcherStatus } from "@/api/types";
import { initialLiveState } from "@/live/reducer";
import { ALL_DEVICES, phoneStorageKey } from "./layout";
import { PhoneSections, PhoneStatus } from "./phone";
import { PhoneActions, PhoneStreamButton } from "./phone-actions";
import { PhoneScenes } from "./phone-scenes";
import { type ObsControls, RecordButton } from "./quick-actions";
import { useLiveLayout } from "./use-live-layout";

let root: Root;
let host: HTMLDivElement;
let client: QueryClient;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  client.clear();
  localStorage.clear();
  vi.unstubAllGlobals();
});
function render(node: ReactNode) {
  act(() =>
    root.render(
      <I18nProvider i18n={i18n}>
        <QueryClientProvider client={client}>{node}</QueryClientProvider>
      </I18nProvider>,
    ),
  );
}
function button(name: string) {
  const element = Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find(
    (node) => node.getAttribute("aria-label") === name || node.textContent === name,
  );
  if (!element) throw new Error(`Missing button: ${name}`);
  return element;
}
const click = (name: string) => act(() => button(name).click());
function SectionsProbe() {
  const layout = useLiveLayout("phone-user", "phone-switcher", 12, ALL_DEVICES);
  return (
    <PhoneSections sections={layout.sections} updateSections={layout.updateSections}>
      {(id) => <p>Content {id}</p>}
    </PhoneSections>
  );
}
const order = () =>
  Array.from(host.querySelectorAll("[data-phone-section]")).map((node) =>
    node.getAttribute("data-phone-section"),
  );
const status: SwitcherStatus = {
  obs_connected: true,
  streaming: true,
  state: "live",
  auto_switch: true,
  browser_source: true,
  recording: false,
  recording_paused: false,
  updated_at: new Date().toISOString(),
  current_scene: { uuid: "scene-1", name: "IRL" },
};
function controls() {
  return {
    stream: { mutate: vi.fn(), isPending: false },
    record: { mutate: vi.fn(), isPending: false },
    fix: { mutate: vi.fn(), isPending: false },
    autoSwitch: { mutate: vi.fn(), isPending: false },
    marker: { mutate: vi.fn(), isPending: false },
  } as unknown as ObsControls;
}

it("reorders and hides phone sections through settings and restores the saved order after remount", () => {
  render(<SectionsProbe />);
  expect(order()).toEqual(["servers", "scenes", "devices", "activity"]);
  click("Live settings");
  expect(button("Move Stream servers up").disabled).toBe(true);
  expect(button("Move Activity down").disabled).toBe(true);
  click("Move Devices up");
  click("Show Stream servers");
  expect(order()).toEqual(["devices", "scenes", "activity"]);
  click("Close");
  render(null);
  render(<SectionsProbe />);
  expect(order()).toEqual(["devices", "scenes", "activity"]);
  expect(
    JSON.parse(localStorage.getItem(phoneStorageKey("phone-user", "phone-switcher")) ?? "null"),
  ).toEqual({
    v: 1,
    sections: [
      { id: "servers", hidden: true },
      { id: "devices", hidden: false },
      { id: "scenes", hidden: false },
      { id: "activity", hidden: false },
    ],
  });
});

it("shows compact recording in the phone status card, outside the four-slot bar, and respects disabled and pending controls", () => {
  const obs = controls();
  const phone = (disabled: boolean) => (
    <>
      <PhoneStatus
        state="live"
        status={status}
        servers={[]}
        activeServer={undefined}
        live={initialLiveState}
        now={Date.now()}
        low={1000}
        offline={300}
        scenesSet
        narrow
        paused={disabled}
        record={
          <RecordButton controls={obs} status={status} disabled={disabled} variant="compact" />
        }
      />
      <PhoneActions
        controls={obs}
        status={status}
        disabled={disabled}
        canControl={!disabled}
        autoSwitchSetting
      />
    </>
  );
  render(phone(false));
  expect(button("Record").closest("section")?.getAttribute("aria-label")).toBe("Stream status");
  expect(host.querySelectorAll('[aria-label="Live controls"] button')).toHaveLength(4);
  expect(button("Record").disabled).toBe(false);
  click("Record");
  expect(obs.record.mutate).toHaveBeenCalledExactlyOnceWith(true);
  render(phone(true));
  expect(button("Record").disabled).toBe(true);
  click("Record");
  expect(obs.record.mutate).toHaveBeenCalledTimes(1);
  obs.record.isPending = true;
  render(phone(false));
  expect(button("Record").disabled).toBe(true);
});

it("requires stop confirmation in a bottom sheet, cancellation does nothing, and confirm stops once", () => {
  const obs = controls();
  render(<PhoneStreamButton controls={obs} status={status} disabled={false} />);
  click("Stop");
  expect(obs.stream.mutate).not.toHaveBeenCalled();
  const dialog = document.querySelector('[role="dialog"]');
  expect(dialog?.textContent).toContain("Stop the stream?");
  expect(dialog?.textContent).toContain("Your viewers will see the stream end.");
  expect(dialog?.className).toContain("bottom-0");
  click("Keep streaming");
  expect(obs.stream.mutate).not.toHaveBeenCalled();
  click("Stop");
  click("Stop stream");
  expect(obs.stream.mutate).toHaveBeenCalledExactlyOnceWith(false);
});

it("confirms starting and disables stream, fix, markers and auto according to the existing gates", () => {
  const obs = controls();
  render(
    <PhoneStreamButton controls={obs} status={{ ...status, streaming: false }} disabled={false} />,
  );
  click("Start");
  expect(document.body.textContent).toContain("OBS goes live with the scene on air now (IRL).");
  click("Start stream");
  expect(obs.stream.mutate).toHaveBeenCalledExactlyOnceWith(true);
  render(
    <PhoneActions controls={obs} status={status} disabled canControl={false} autoSwitchSetting />,
  );
  expect(button("Stop").disabled).toBe(true);
  expect(button("Fix feed").disabled).toBe(true);
  expect(button("Marker").disabled).toBe(true);
  expect(button("Auto switch").disabled).toBe(true);
  click("Stop");
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});

it("shows six scenes, filters every scene in the sheet including hidden scenes, and respects control permission", () => {
  localStorage.setItem("noalbs-hidden-scenes:s", JSON.stringify(["scene-8"]));
  client.setQueryData(obsScenesQuery("w", "s").queryKey, {
    scenes: Array.from({ length: 9 }, (_, index) => ({
      uuid: `scene-${index}`,
      name: `Scene ${index}`,
    })),
    current_scene_uuid: "scene-1",
    current_scene_name: "Scene 1",
  });
  const scenes = (
    <PhoneScenes
      path={{ workspace_id: "w", switcher_id: "s" }}
      roles={{ live: "scene-1" }}
      currentUuid="scene-1"
      currentName="Scene 1"
      canControl={false}
      obsConnected
      streaming
      feedState="live"
    />
  );
  render(scenes);
  expect(host.querySelectorAll("button[aria-pressed]")).toHaveLength(6);
  expect(host.querySelector('button[aria-pressed="true"]')?.textContent).toContain("On air");
  click("All 9 scenes");
  const input = document.querySelector<HTMLInputElement>('input[aria-label="Filter scenes"]');
  act(() => {
    if (!input) throw new Error("Missing scene filter");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(
      input,
      "Scene 8",
    );
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(
    document.querySelector('[role="dialog"]')?.querySelectorAll("button[aria-pressed]"),
  ).toHaveLength(1);
  expect(button("Scene 8No role").disabled).toBe(true);
});

it("renders phone status variants without fabricating metrics or BRB countdowns", () => {
  const props = {
    status,
    servers: [],
    activeServer: undefined,
    live: initialLiveState,
    now: Date.now(),
    low: 1000,
    offline: 300,
    scenesSet: true,
    narrow: true,
    paused: false,
  };
  const text = (node: ReactNode) =>
    renderToStaticMarkup(<I18nProvider i18n={i18n}>{node}</I18nProvider>);
  for (const state of [
    "connecting",
    "reconnecting",
    "paused",
    "suspended",
    "browserOffline",
    "obsUnreachable",
    "notStreaming",
    "noFeed",
    "low",
    "live",
  ] as const) {
    const html = text(
      <PhoneStatus {...props} state={state} paused={state === "paused" || state === "suspended"} />,
    );
    expect(html).toContain("Stream status");
    if (state === "noFeed") {
      expect(html).toContain("BRB");
      expect(html).toContain("No feed");
      expect(html).not.toContain("stream stops");
    }
    if (state === "obsUnreachable" || state === "browserOffline")
      expect(html).toContain("OBS is offline");
    if (state === "notStreaming") expect(html).toContain("In OBS");
    if (state === "reconnecting") expect(html).toContain("opacity-50");
    if (state === "paused" || state === "suspended") expect(html).toContain("Paused");
  }
});
