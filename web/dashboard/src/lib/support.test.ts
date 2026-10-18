import { describe, expect, it } from "vitest";
import {
  describeSupportAccess,
  isSupportChange,
  supportAccessBasis,
  supportGated,
} from "./support";

const switcher = (route: string) => `/workspaces/{workspace_id}/switchers/{switcher_id}${route}`;

describe("describeSupportAccess", () => {
  it("names what support did on a switcher", () => {
    expect(describeSupportAccess({ method: "POST", route: switcher("/obs/scene") })).toBe(
      "Switched scene",
    );
    expect(describeSupportAccess({ method: "GET", route: switcher("/obs/screenshot") })).toBe(
      "Watched the OBS preview",
    );
    expect(
      describeSupportAccess({
        method: "PATCH",
        route: switcher("/stream-servers/{stream_server_id}"),
      }),
    ).toBe("Changed a stream server");
    expect(describeSupportAccess({ method: "PATCH", route: switcher("") })).toBe(
      "Renamed the switcher",
    );
  });

  it("groups remote controls and chat", () => {
    expect(describeSupportAccess({ method: "PUT", route: switcher("/moblin/zoom") })).toBe(
      "Controlled Moblin",
    );
    expect(describeSupportAccess({ method: "POST", route: switcher("/belabox/stream") })).toBe(
      "Controlled BELABOX",
    );
    expect(describeSupportAccess({ method: "PUT", route: switcher("/chat/settings") })).toBe(
      "Changed chat settings",
    );
  });

  it("falls back to the route for changes it doesn't know", () => {
    expect(
      describeSupportAccess({ method: "GET", route: "/workspaces/{workspace_id}/chat-bot" }),
    ).toBe("Looked around");
    expect(
      describeSupportAccess({ method: "POST", route: "/workspaces/{workspace_id}/something" }),
    ).toBe("Changed something (POST /something)");
  });
});

describe("isSupportChange", () => {
  it("counts everything but reads", () => {
    expect(isSupportChange({ method: "GET" })).toBe(false);
    expect(isSupportChange({ method: "DELETE" })).toBe(true);
  });
});

describe("refused attempts", () => {
  it("are named as attempts", () => {
    expect(describeSupportAccess({ method: "POST", route: switcher("/connect-token") })).toBe(
      "Tried to create a new browser-source link",
    );
    expect(
      describeSupportAccess({ method: "PUT", route: "/workspaces/{workspace_id}/support-access" }),
    ).toBe("Tried to change support access");
  });
});

describe("workspace routes", () => {
  it("aren't mistaken for switcher ones", () => {
    expect(describeSupportAccess({ method: "DELETE", route: "/workspaces/{workspace_id}" })).toBe(
      "Deleted the workspace",
    );
    expect(describeSupportAccess({ method: "DELETE", route: switcher("") })).toBe(
      "Deleted the switcher",
    );
    expect(
      describeSupportAccess({ method: "GET", route: "/workspaces/{workspace_id}/switchers" }),
    ).toBe("Looked at the switchers");
  });
});

describe("supportAccessBasis", () => {
  const entry = {
    grant_level: null,
    break_glass: false,
    grant_in_force: null,
    route: "/workspaces/{workspace_id}/switchers",
  } as const;

  it("tells permission, emergency and older entries apart", () => {
    expect(supportAccessBasis({ ...entry, grant_level: "view" }).label).toBe("With permission");
    expect(supportAccessBasis({ ...entry, grant_level: "view", break_glass: true }).label).toBe(
      "Emergency access",
    );
    expect(supportAccessBasis({ ...entry, grant_in_force: "control" }).label).toBe(
      "Beyond your permission",
    );
    expect(supportAccessBasis(entry).label).toBe("Without permission");
  });
});

describe("older entries", () => {
  it("name billing", () => {
    expect(
      describeSupportAccess({
        method: "POST",
        route: "/workspaces/{workspace_id}/billing/checkout",
      }),
    ).toBe("Started a checkout");
  });
});

describe("staff actions in the workspace log", () => {
  it("are named and badged as staff actions", () => {
    const route = "/admin/workspaces/{workspace_id}/suspension";
    expect(describeSupportAccess({ method: "PUT", route })).toBe("Suspended the workspace");
    expect(describeSupportAccess({ method: "DELETE", route })).toBe("Lifted the suspension");
    expect(
      supportAccessBasis({ grant_level: null, break_glass: false, grant_in_force: null, route })
        .label,
    ).toBe("Staff action");
  });
});

describe("supportGated", () => {
  const workspace = { role: null, support_access: null, organization_role: null } as const;

  it("gates staff outside the workspace until it grants access", () => {
    expect(supportGated(workspace)).toBe(true);
    expect(supportGated({ ...workspace, role: "admin" })).toBe(false);
  });

  it("lets the owning organization's team in without a grant", () => {
    expect(supportGated({ ...workspace, organization_role: "member" })).toBe(false);
  });
});
