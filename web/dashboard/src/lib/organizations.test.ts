import { describe, expect, it } from "vitest";
import {
  allocationLabel,
  belowUsage,
  bulkOutcome,
  clientState,
  csvCell,
  customerInviteUrl,
  DEFAULT_ALLOCATION,
  digitsOnly,
  invitesCsv,
  isClaimed,
  LIMIT_DEFAULTS,
  limitsForm,
  liveTag,
  otherLimits,
  parseActivitySearch,
  parseAllocation,
  parseClientsSearch,
  parseLimits,
  parseLiveSearch,
  parseNames,
  poolUsage,
  teamInviteUrl,
} from "./organizations";

const organization = (switchers: number, max: number) => ({
  limits: {
    max_switchers: max,
    max_stream_servers: 5,
    max_members: 3,
    history_days: 30,
  },
  usage: { clients: 2, switchers, team: 1 },
});

describe("poolUsage", () => {
  it("counts what is left in the pool", () => {
    expect(poolUsage(organization(3, 10))).toEqual({
      used: 3,
      max: 10,
      remaining: 7,
      over: 0,
      full: false,
    });
  });

  it("is full at the limit and shows switchers over a lowered pool", () => {
    expect(poolUsage(organization(10, 10)).full).toBe(true);
    expect(poolUsage(organization(12, 10))).toMatchObject({ remaining: 0, over: 2, full: true });
  });
});

describe("clientState", () => {
  const base = { streaming: false, obs_connected: false, suspended: false };

  it("shows the most telling state first", () => {
    expect(clientState(base)).toBe("offline");
    expect(clientState({ ...base, obs_connected: true })).toBe("online");
    expect(clientState({ ...base, obs_connected: true, streaming: true })).toBe("live");
    expect(clientState({ ...base, streaming: true, suspended: true })).toBe("suspended");
  });
});

describe("parseAllocation", () => {
  it("reads an empty field as no cap and whole numbers as the cap", () => {
    expect(parseAllocation("")).toBeNull();
    expect(parseAllocation("  ")).toBeNull();
    expect(parseAllocation("0")).toBe(0);
    expect(parseAllocation(" 4 ")).toBe(4);
    expect(parseAllocation("10000")).toBe(10_000);
  });

  it("starts a new client at one switcher, which is a valid allocation", () => {
    expect(DEFAULT_ALLOCATION).toBe(1);
    expect(parseAllocation(String(DEFAULT_ALLOCATION))).toBe(1);
  });

  it("refuses anything else", () => {
    for (const text of ["-1", "1.5", "two", "10001", "1e3"]) {
      expect(parseAllocation(text)).toBeUndefined();
    }
  });
});

describe("labels and links", () => {
  it("says the pool limits a client without a cap", () => {
    expect(allocationLabel(null)).toBe("Pool");
    expect(allocationLabel(undefined)).toBe("Pool");
    expect(allocationLabel(3)).toBe("3");
  });

  it("knows whether a customer joined", () => {
    expect(isClaimed({ customers: 0 })).toBe(false);
    expect(isClaimed({ customers: 1 })).toBe(true);
  });

  it("builds the invite links", () => {
    expect(customerInviteUrl("https://noalbs.com", "abc")).toBe("https://noalbs.com/invite/abc");
    expect(teamInviteUrl("https://noalbs.com", "abc")).toBe("https://noalbs.com/org-invite/abc");
  });
});

describe("parseLimits", () => {
  const form = { ...LIMIT_DEFAULTS, maxSwitchers: "25" };

  it("reads a valid form", () => {
    expect(parseLimits(form)).toEqual({
      max_switchers: 25,
      max_stream_servers: 10,
      max_members: 3,
      history_days: 90,
    });
  });

  it("round-trips through the form", () => {
    const limits = parseLimits(form);
    expect(limits && parseLimits(limitsForm(limits))).toEqual(limits);
  });

  it("refuses text, negatives and out-of-range values", () => {
    for (const bad of ["", "ten", "-1", "1.5", "100001"]) {
      expect(parseLimits({ ...form, maxSwitchers: bad })).toBeNull();
    }
    expect(parseLimits({ ...form, historyDays: "3651" })).toBeNull();
    expect(parseLimits({ ...form, maxMembers: "1001" })).toBeNull();
  });
});

describe("belowUsage", () => {
  it("allows a limit at or above what is in use, and no cap", () => {
    expect(belowUsage(3, 3)).toBe(false);
    expect(belowUsage(10, 3)).toBe(false);
    expect(belowUsage(null, 3)).toBe(false);
    expect(belowUsage(undefined, 3)).toBe(false);
    expect(belowUsage(0, 0)).toBe(false);
  });

  it("refuses a limit under it", () => {
    expect(belowUsage(2, 3)).toBe(true);
    expect(belowUsage(0, 1)).toBe(true);
  });
});

describe("parseClientsSearch", () => {
  it("keeps what fits and drops the rest", () => {
    expect(parseClientsSearch({ q: "nomad", page: "3", customer: "joined", sort: "name" })).toEqual(
      { q: "nomad", page: 3, customer: "joined", sort: "name" },
    );
    expect(parseClientsSearch({ q: "", page: "1", customer: "everyone", sort: "newest" })).toEqual({
      q: undefined,
      page: undefined,
      customer: undefined,
      sort: undefined,
    });
    expect(parseClientsSearch({ page: "2.5" }).page).toBeUndefined();
    expect(parseClientsSearch({ q: 5 }).q).toBeUndefined();
  });
});

describe("otherLimits", () => {
  it("lists every limit besides the pool", () => {
    const limits = { ...organization(0, 10).limits, history_days: 1 };
    expect(otherLimits(limits).map((limit) => [limit.label, limit.value])).toEqual([
      ["Stream servers", "5"],
      ["Customers", "3"],
      ["History", "1 day"],
    ]);
    expect(otherLimits({ ...limits, history_days: 30 })[2]?.value).toBe("30 days");
  });
});

describe("digitsOnly", () => {
  it("drops everything but digits, typed or pasted", () => {
    expect(digitsOnly("12")).toBe("12");
    expect(digitsOnly("1e3")).toBe("13");
    expect(digitsOnly("-4.5 ")).toBe("45");
    expect(digitsOnly("two")).toBe("");
  });
});

describe("parseClientsSearch state", () => {
  it("keeps a known state only", () => {
    expect(parseClientsSearch({ state: "live" }).state).toBe("live");
    expect(parseClientsSearch({ state: "suspended" }).state).toBe("suspended");
    expect(parseClientsSearch({ state: "asleep" }).state).toBeUndefined();
  });
});

describe("parseNames", () => {
  it("takes one name per line and drops blanks", () => {
    expect(parseNames("Kate Live\n\n  Bike Crew  \r\nNomad")).toEqual({
      names: ["Kate Live", "Bike Crew", "Nomad"],
      tooMany: false,
    });
    expect(parseNames("  \n ")).toEqual({ names: [], tooMany: false });
  });

  it("stops at what a bulk action takes", () => {
    const text = Array.from({ length: 101 }, (_, index) => `Client ${index}`).join("\n");
    const parsed = parseNames(text);
    expect(parsed.names).toHaveLength(100);
    expect(parsed.tooMany).toBe(true);
  });
});

describe("bulkOutcome", () => {
  it("counts what worked and what didn't", () => {
    expect(bulkOutcome([{ error: null }, { error: { code: "x" } }, {}])).toEqual({
      ok: 2,
      failed: 1,
    });
  });
});

describe("invitesCsv", () => {
  it("lists the links made, quoting what needs it, and skips the failed ones", () => {
    const csv = invitesCsv(
      [
        { name: 'Kate, "the" Streamer', url_slug: "kate", token: "t1", expires_at: "2026-10-23" },
        { name: "No room", url_slug: "full", token: null },
      ],
      "https://noalbs.example",
    );
    expect(csv).toBe(
      [
        "client,url,invite link,expires",
        '"Kate, ""the"" Streamer",kate,https://noalbs.example/invite/t1,2026-10-23',
      ].join("\n"),
    );
  });
});

describe("activity and live addresses", () => {
  it("read a preset, a client and a search", () => {
    expect(parseActivitySearch({ q: "kate", show: "streams", client: "abc" })).toEqual({
      q: "kate",
      show: "streams",
      client: "abc",
    });
    expect(parseActivitySearch({ show: "all" }).show).toBeUndefined();
    expect(parseActivitySearch({ show: "nonsense" }).show).toBeUndefined();
  });

  it("read the live page and its filter", () => {
    expect(parseLiveSearch({ q: "k", page: "2", show: "obs" })).toEqual({
      q: "k",
      page: 2,
      show: "obs",
    });
    expect(parseLiveSearch({ page: "1", show: "all" })).toEqual({
      q: undefined,
      page: undefined,
      show: undefined,
    });
  });
});

describe("liveTag", () => {
  const item = { paused: false, obs_connected: true, streaming: true, state: "live" as const };

  it("tells live, low and BRB apart", () => {
    expect(liveTag(item).tag).toBe("LIVE");
    expect(liveTag({ ...item, state: "low" }).tag).toBe("LOW");
    expect(liveTag({ ...item, state: "offline" }).tag).toBe("BRB");
  });

  it("says what a switcher that isn't streaming is", () => {
    expect(liveTag({ ...item, streaming: false, state: "idle" }).tag).toBe("IDLE");
    expect(liveTag({ ...item, obs_connected: false, streaming: false }).tag).toBe("NO OBS");
    expect(liveTag({ ...item, paused: true }).tag).toBe("PAUSED");
  });
});

describe("csvCell", () => {
  it("keeps a name from running as a spreadsheet formula", () => {
    expect(csvCell("Kate Live")).toBe("Kate Live");
    expect(csvCell('=HYPERLINK("http://x")')).toBe('"\'=HYPERLINK(""http://x"")"');
    expect(csvCell("+1")).toBe("'+1");
    expect(csvCell("-2")).toBe("'-2");
    expect(csvCell("@sum")).toBe("'@sum");
    expect(csvCell("a=b")).toBe("a=b");
  });
});
