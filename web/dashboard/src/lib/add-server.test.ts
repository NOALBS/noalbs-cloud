import { describe, expect, it } from "vitest";
import {
  type AddServerRequest,
  buildAddServerFragment,
  detailFieldsOf,
  displayHost,
  parseAddServerFragment,
} from "./add-server";

describe("parseAddServerFragment", () => {
  it("reads kind, url and name, with or without the #", () => {
    const hash = "kind=belabox&url=https%3A%2F%2Fstats.srt.belabox.net%2Fabc123&name=My%20Bela";
    const expected = {
      kind: "belabox",
      url: "https://stats.srt.belabox.net/abc123",
      name: "My Bela",
      details: {},
    };
    expect(parseAddServerFragment(`#${hash}`)).toEqual(expected);
    expect(parseAddServerFragment(hash)).toEqual(expected);
  });

  it("treats + as a space and a missing name as null", () => {
    expect(parseAddServerFragment("kind=sls&url=http://a/stats&name=a+b")?.name).toBe("a b");
    expect(parseAddServerFragment("kind=sls&url=http://a/stats")?.name).toBeNull();
  });

  it("needs a known kind and a url", () => {
    expect(parseAddServerFragment("")).toBeNull();
    expect(parseAddServerFragment("url=http://a")).toBeNull();
    expect(parseAddServerFragment("kind=belabox")).toBeNull();
    expect(parseAddServerFragment("kind=wowza&url=http://a")).toBeNull();
    expect(parseAddServerFragment(`kind=belabox&url=http://a/${"x".repeat(2048)}`)).toBeNull();
  });

  it("keeps only the settings the kind uses", () => {
    const request = parseAddServerFragment(
      "kind=sls&url=http://a/stats&publisher=live/feed&api_key=k&application=app&password=p",
    );
    expect(request?.details).toEqual({ publisher: "live/feed", api_key: "k" });
  });

  it("reads basic auth for kinds that take it", () => {
    const request = parseAddServerFragment("kind=mediamtx&url=http://a&username=u&password=p");
    expect(request?.details).toEqual({ username: "u", password: "p" });
  });

  it("caps the name", () => {
    const request = parseAddServerFragment(`kind=sls&url=http://a&name=${"n".repeat(100)}`);
    expect(request?.name).toHaveLength(60);
  });
});

describe("buildAddServerFragment", () => {
  it("round-trips", () => {
    const request: AddServerRequest = {
      kind: "nginx",
      url: "http://host/stat?a=1&b=2#x",
      name: "Home & away",
      details: { application: "live", key: "feed 1" },
    };
    expect(parseAddServerFragment(buildAddServerFragment(request))).toEqual(request);
  });

  it("leaves out empty and unused fields", () => {
    const fragment = buildAddServerFragment({
      kind: "belabox",
      url: "http://a",
      name: " ",
      details: { key: "nope" },
    });
    expect(fragment).toBe("kind=belabox&url=http%3A%2F%2Fa");
  });
});

describe("detailFieldsOf", () => {
  it("lists the fields of a kind", () => {
    expect(detailFieldsOf("belabox")).toEqual([]);
    expect(detailFieldsOf("nms")).toEqual(["application", "key", "username", "password"]);
    expect(detailFieldsOf("irlhosting_websocket")).toEqual(["publisher", "api_key"]);
  });
});

describe("displayHost", () => {
  it("shows the host of web and socket URLs only", () => {
    expect(displayHost("https://stats.srt.belabox.net/abc123")).toBe("stats.srt.belabox.net");
    expect(displayHost("wss://ingest.example:8443/ws")).toBe("ingest.example:8443");
    expect(displayHost("javascript:alert(1)")).toBeNull();
    expect(displayHost("not a url")).toBeNull();
  });
});
