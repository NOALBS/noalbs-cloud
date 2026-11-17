import { i18n } from "@lingui/core";
import { I18nProvider } from "@lingui/react";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SwitcherStatus } from "@/api/types";
import { StreamButton, useObsControls } from "./quick-actions";

const mutation = vi.hoisted(() => ({ isPending: false, mutate: vi.fn() }));
vi.mock("@tanstack/react-query", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-query")>()),
  useQueryClient: () => ({ invalidateQueries: vi.fn() }),
  useMutation: () => mutation,
}));

let host: HTMLDivElement;
let root: Root;
const offAir: SwitcherStatus = {
  auto_switch: true,
  browser_source: true,
  obs_connected: true,
  recording: false,
  recording_paused: false,
  state: "live",
  streaming: false,
  updated_at: new Date().toISOString(),
};

function Subject({
  disabled = false,
  variant = "compact",
  status,
}: {
  disabled?: boolean;
  variant?: "compact" | "tile";
  status?: SwitcherStatus | null;
}) {
  const controls = useObsControls({ workspace_id: "workspace", switcher_id: "switcher" });
  return <StreamButton controls={controls} status={status} disabled={disabled} variant={variant} />;
}

const render = (props: Parameters<typeof Subject>[0] = {}) => {
  act(() =>
    root.render(
      <I18nProvider i18n={i18n}>
        <Subject {...props} />
      </I18nProvider>,
    ),
  );
  const button = host.querySelector("button");
  if (!button) throw new Error("Missing stream button");
  return button;
};

describe("Start stream", () => {
  beforeEach(() => {
    (
      globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    mutation.isPending = false;
    mutation.mutate.mockClear();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
  });

  it.each(["compact", "tile"] as const)("uses the primary variant in %s view", (variant) => {
    const button = render({ variant, status: offAir });
    expect(button.disabled).toBe(false);
    expect(button.dataset.variant).toBe("default");
    expect(button.classList.contains("bg-primary")).toBe(true);
    expect(button.classList.contains("text-primary-foreground")).toBe(true);
    expect(button.classList.contains("hover:bg-primary/90")).toBe(true);
    expect(button.classList.contains("dark:bg-input/30")).toBe(false);
    expect(button.classList.contains("dark:hover:bg-input/50")).toBe(false);
    expect(button.classList.contains("hover:text-accent-foreground")).toBe(false);
  });

  it("still asks for confirmation before starting", () => {
    const button = render();
    act(() => button.click());
    expect(document.querySelector('[role="alertdialog"]')?.textContent).toContain(
      "Start the stream?",
    );
    expect(mutation.mutate).not.toHaveBeenCalled();
    const confirm = [
      ...document.querySelectorAll<HTMLButtonElement>('[role="alertdialog"] button'),
    ].find((node) => node.textContent === "Start stream");
    act(() => confirm?.click());
    expect(mutation.mutate).toHaveBeenCalledWith(true);
  });

  it.each([null, undefined])("keeps the primary disabled for unavailable status %s", (status) => {
    const button = render({ disabled: true, status });
    expect(button.disabled).toBe(true);
    expect(button.dataset.variant).toBe("default");
    expect(button.classList.contains("disabled:opacity-50")).toBe(true);
    act(() => button.click());
    expect(document.querySelector('[role="alertdialog"]')).toBeNull();
    expect(mutation.mutate).not.toHaveBeenCalled();
  });

  it("disables only during a pending stream mutation and recovers when it finishes", () => {
    mutation.isPending = true;
    expect(render().disabled).toBe(true);
    mutation.isPending = false;
    expect(render().disabled).toBe(false);
  });
});
