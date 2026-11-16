import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useMetersSetting } from "@/lib/audio";

let host: HTMLDivElement;
let root: Root;

function Setting({ defaultOn }: { defaultOn: boolean }) {
  const [on, setOn] = useMetersSetting(defaultOn);
  return (
    <button type="button" onClick={() => setOn(!on)}>
      {on ? "on" : "off"}
    </button>
  );
}

beforeEach(() => {
  (
    globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  localStorage.clear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

describe("audio meter preference", () => {
  it.each([true, false])("uses the layout default %s without a saved choice", (defaultOn) => {
    act(() => root.render(<Setting defaultOn={defaultOn} />));
    expect(host.textContent).toBe(defaultOn ? "on" : "off");
  });
  it.each(["on", "off"])("preserves an explicit %s choice", (choice) => {
    localStorage.setItem("noalbs-audio-meters", choice);
    act(() => root.render(<Setting defaultOn={choice === "off"} />));
    expect(host.textContent).toBe(choice);
  });
  it("keeps an explicit opt-out after the layout default changes", () => {
    act(() => root.render(<Setting defaultOn />));
    act(() => host.querySelector("button")?.click());
    expect(localStorage.getItem("noalbs-audio-meters")).toBe("off");
    act(() => root.render(<Setting defaultOn={false} />));
    act(() => root.render(<Setting defaultOn />));
    expect(host.textContent).toBe("off");
  });
  it("uses the default and keeps working when storage is unavailable", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("unavailable");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("unavailable");
    });
    act(() => root.render(<Setting defaultOn />));
    expect(host.textContent).toBe("on");
    act(() => host.querySelector("button")?.click());
    expect(host.textContent).toBe("off");
  });
});
