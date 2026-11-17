import { i18n } from "@lingui/core";
import { I18nProvider } from "@lingui/react";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ObsSource, ObsSources } from "@/api/types";
import { SourceList } from "./source-list";

const query = vi.hoisted(() => ({
  data: undefined as ObsSources | undefined,
  isPending: false,
  isError: false,
  error: new Error("Source load failed"),
}));
const mutate = vi.hoisted(() => vi.fn());
vi.mock("@tanstack/react-query", async (original) => ({
  ...(await original<typeof import("@tanstack/react-query")>()),
  useQuery: () => query,
  useQueryClient: () => ({ invalidateQueries: vi.fn() }),
  useMutation: () => ({ isPending: false, mutate }),
}));

let host: HTMLDivElement;
let root: Root;
const source = (name: string, item_id: number, depth = 0, is_group = false): ObsSource => ({
  name,
  item_id,
  depth,
  is_group,
  parent: depth === 2 ? "Widgets" : depth === 1 ? "Overlay" : "Live",
  parent_is_group: depth > 0,
  is_scene: false,
  enabled: item_id !== 2,
  input_kind: is_group ? null : "browser_source",
});
const render = () =>
  act(() =>
    root.render(
      <I18nProvider i18n={i18n}>
        <SourceList
          path={{ workspace_id: "workspace", switcher_id: "switcher" }}
          scene="Live"
          canControl
          canEditSources
          obsConnected
          columns
        />
      </I18nProvider>,
    ),
  );
const collapse = () => host.querySelector<HTMLButtonElement>("[aria-expanded]");
const search = (value: string) =>
  act(() => {
    const input = host.querySelector<HTMLInputElement>('input[type="search"]');
    if (!input) throw new Error("Missing source search");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });

describe("source tree rows", () => {
  beforeEach(() => {
    (
      globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    localStorage.clear();
    mutate.mockClear();
    query.isError = false;
    query.data = {
      scene: "Live",
      sources: [
        source("Overlay", 1, 0, true),
        source("Widgets", 2, 1, true),
        source("Countdown with chat and supporters", 3, 2),
        ...Array.from({ length: 6 }, (_, i) => source(`Camera ${i}`, i + 4)),
      ],
    };
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
  });
  it("renders each group once, with inline chevrons and progressively indented children", () => {
    render();
    expect(host.querySelectorAll("h3")).toHaveLength(0);
    expect(host.querySelectorAll('[title="Overlay"]')).toHaveLength(1);
    const rows = host.querySelectorAll<HTMLElement>("[data-source-depth]");
    expect(
      Array.from(rows)
        .slice(0, 3)
        .map((row) => row.style.paddingInlineStart),
    ).toEqual(["0px", "16px", "32px"]);
    expect(collapse()?.parentElement).toBe(rows[0]);
    expect(rows[0]?.querySelector("button[aria-pressed]")).not.toBeNull();
    expect(collapse()?.classList.contains("border-0")).toBe(true);
    expect(host.querySelectorAll(".grid-cols-2")).toHaveLength(0);
  });
  it("keeps matching ancestors in the same tree and restores collapsed branches after search", () => {
    render();
    act(() => collapse()?.click());
    expect(host.querySelectorAll("[data-source-depth]")).toHaveLength(7);
    search("countdown");
    expect(host.querySelectorAll("[data-source-depth]")).toHaveLength(3);
    expect(host.querySelectorAll("h3")).toHaveLength(0);
    expect(host.querySelector('[title="Widgets"]')).not.toBeNull();
    expect(collapse()?.getAttribute("aria-expanded")).toBe("true");
    search("unmatched");
    expect(host.textContent).toContain("No source matches");
    search("");
    expect(host.querySelectorAll("[data-source-depth]")).toHaveLength(7);
    expect(collapse()?.getAttribute("aria-expanded")).toBe("false");
  });
  it("retains the independent eye action and the full-name tooltip on a child", () => {
    render();
    const name = host.querySelector('[title="Countdown with chat and supporters"]');
    const button = name?.closest("button");
    expect(name?.classList.contains("truncate")).toBe(true);
    act(() => button?.click());
    expect(mutate).toHaveBeenCalledWith({ source: query.data?.sources[2], enabled: false });
  });
  it("retains empty and failed-load states without placeholder tree rows", () => {
    query.data = { scene: "Live", sources: [] };
    render();
    expect(host.textContent).toContain("This scene has no sources.");
    expect(host.querySelectorAll("[data-source-depth]")).toHaveLength(0);
    query.data = undefined;
    query.isError = true;
    render();
    expect(host.textContent).toContain("Source load failed");
  });
});
