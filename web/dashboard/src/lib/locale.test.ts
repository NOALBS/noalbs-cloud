import { describe, expect, it } from "vitest";
import { currentLocale, parsePreference, pickLocale, resolveLocale } from "./locale";

describe("parsePreference", () => {
  it("keeps a language we have", () => {
    expect(parsePreference("en")).toBe("en");
  });

  it("falls back to following the browser", () => {
    expect(parsePreference(null)).toBe("system");
    expect(parsePreference("tlh")).toBe("system");
    expect(parsePreference("system")).toBe("system");
  });
});

describe("pickLocale", () => {
  it("matches on the language alone", () => {
    expect(pickLocale(["en-GB"])).toBe("en");
  });

  it("matches a regional language exactly", () => {
    expect(pickLocale(["pt-BR"])).toBe("pt-br");
    expect(pickLocale(["zh-TW"])).toBe("zh-tw");
  });

  it("gives another region the language we have", () => {
    expect(pickLocale(["nl-BE"])).toBe("nl");
    expect(pickLocale(["pt-PT"])).toBe("pt-br");
  });

  it("picks Chinese by script", () => {
    expect(pickLocale(["zh-CN"])).toBe("zh-cn");
    expect(pickLocale(["zh-SG"])).toBe("zh-cn");
    expect(pickLocale(["zh"])).toBe("zh-cn");
    expect(pickLocale(["zh-HK"])).toBe("zh-tw");
    expect(pickLocale(["zh-Hant-TW"])).toBe("zh-tw");
  });

  it("skips languages we don't have and falls back to English", () => {
    expect(pickLocale(["tlh", "en-US"])).toBe("en");
    expect(pickLocale(["tlh"])).toBe("en");
    expect(pickLocale([])).toBe("en");
  });
});

describe("resolveLocale", () => {
  it("lets a picked language win over the browser's", () => {
    expect(resolveLocale("en", ["tlh"])).toBe("en");
  });

  it("follows the browser on system", () => {
    expect(resolveLocale("system", ["en-US"])).toBe("en");
  });
});

it("formats in English before any catalog is active", () => {
  expect(currentLocale()).toBe("en");
});
