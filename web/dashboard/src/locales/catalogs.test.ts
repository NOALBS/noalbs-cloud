import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { LOCALES } from "@/lib/locale";

const catalogPath = (locale: string) => resolve("src/locales", locale, "messages.po");

/** The catalog's `msgid`/`msgstr` pairs, without the header. */
function messages(locale: string): [string, string][] {
  const pairs: [string, string][] = [];
  for (const entry of readFileSync(catalogPath(locale), "utf8").split(/\n\n+/).slice(1)) {
    const id = entry.match(/^msgid (".*")$/m)?.[1];
    const text = entry.match(/^msgstr (".*")$/m)?.[1];
    if (id && text) pairs.push([JSON.parse(id), JSON.parse(text)]);
  }
  return pairs;
}

/** What a message fills in or marks up: `{name}`, `{count, plural,`, `<0>`, `</0>`. */
function holes(message: string): string[] {
  const names = [...message.matchAll(/\{(\w+)[,}]/g)].map((match) => match[1] ?? "");
  const tags = message.match(/<\/?\d+\/?>/g) ?? [];
  return [...new Set(names), ...tags].sort();
}

const translated = LOCALES.filter((locale) => locale.code !== "en");

describe("Catalogs", () => {
  it("are there for every language", () => {
    for (const locale of LOCALES) expect(existsSync(catalogPath(locale.code))).toBe(true);
  });

  it("hold the same messages as the source language", () => {
    const source = messages("en").map(([id]) => id);
    for (const locale of translated) {
      expect(messages(locale.code).map(([id]) => id)).toEqual(source);
    }
  });

  it("keep the placeholders and tags in every translation", () => {
    for (const locale of translated) {
      for (const [id, text] of messages(locale.code)) {
        if (text) expect([locale.code, id, holes(text)]).toEqual([locale.code, id, holes(id)]);
      }
    }
  });
});
