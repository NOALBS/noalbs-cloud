import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { Row, RowList } from "./row-list";
import { SegmentedControl } from "./segmented-control";
import { SettingsRow } from "./settings-row";
import { UsageBar } from "./usage-bar";

function Control() {
  const [value, setValue] = useState("operator");
  return (
    <SegmentedControl
      label="Role"
      value={value}
      onValueChange={setValue}
      options={[
        { value: "operator", label: "Operator" },
        { value: "admin", label: "Admin" },
      ]}
    />
  );
}

describe("settings primitives", () => {
  it("renders divided semantic rows with optional content", () => {
    const html = renderToStaticMarkup(
      <RowList>
        <Row title="Member" meta="Joined today" lead={<span>ME</span>}>
          <button type="button">Remove</button>
        </Row>
        <Row title="Empty" />
      </RowList>,
    );
    const container = document.createElement("div");
    container.innerHTML = html;
    expect(container.querySelectorAll("ul > li")).toHaveLength(2);
    expect(container.textContent).toContain("Joined today");
    expect(container.querySelector("button")?.textContent).toBe("Remove");
  });

  it("keeps labels, descriptions and error content with their control", () => {
    const html = renderToStaticMarkup(
      <SettingsRow label={<label htmlFor="name">Name</label>} description="Workspace name">
        <input id="name" aria-invalid="true" />
        <p role="alert">Could not save</p>
      </SettingsRow>,
    );
    expect(html).toContain('for="name"');
    expect(html).toContain('aria-invalid="true"');
    expect(html).toContain("Could not save");
    expect(renderToStaticMarkup(<SettingsRow label="URL" />)).toContain("URL");
  });

  it("opts compact actions into inline phone layout without changing wide controls", () => {
    const container = document.createElement("div");
    container.innerHTML = renderToStaticMarkup(
      <RowList>
        <Row title="Session" compactAction>
          <button type="button">Sign out</button>
        </Row>
        <Row title="Member">
          <select aria-label="Role">
            <option>Admin</option>
          </select>
        </Row>
        <Row title="Empty" compactAction />
      </RowList>,
    );
    const rows = container.querySelectorAll("li");
    expect(rows[0]?.lastElementChild?.className).toContain("shrink-0");
    expect(rows[0]?.lastElementChild?.className).not.toContain("w-full");
    expect(rows[1]?.lastElementChild?.className).toContain("w-full");
    expect(rows[2]?.children).toHaveLength(1);
  });

  it("uses independent native radio groups and calls the controlled change handler", async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const container = document.createElement("div");
    document.body.append(container);
    const root = createRoot(container);
    await act(async () =>
      root.render(
        <>
          <Control />
          <Control />
        </>,
      ),
    );
    const radios = container.querySelectorAll<HTMLInputElement>('input[type="radio"]');
    const [operator, admin, otherOperator] = radios;
    if (!operator || !admin || !otherOperator) throw new Error("Missing role radios");
    expect(operator.checked).toBe(true);
    expect(operator.name).not.toBe(otherOperator.name);
    await act(async () => admin.click());
    expect(admin.checked).toBe(true);
    expect(operator.checked).toBe(false);
    expect(otherOperator.checked).toBe(true);
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  it("keeps usage neutral at zero, at and beyond limits, and without a limit", () => {
    for (const used of [0, 3, 4]) {
      const html = renderToStaticMarkup(<UsageBar label="Members" used={used} max={3} />);
      expect(html).toContain(`Members: ${used} of 3`);
      expect(html).toContain("bg-foreground/55");
      expect(html).not.toContain("bg-low");
    }
    expect(renderToStaticMarkup(<UsageBar label="Members" used={0} max={null} />)).toContain(
      "Members",
    );
    expect(renderToStaticMarkup(<UsageBar label="Members" used={0} max={0} />)).not.toContain(
      "NaN",
    );
  });
});
