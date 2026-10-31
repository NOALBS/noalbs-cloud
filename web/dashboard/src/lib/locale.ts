import { i18n } from "@lingui/core";
import { useSyncExternalStore } from "react";

/** The languages the dashboard is translated into. Keep in step with `locales` in lingui.config.ts. */
export const LOCALES = [
  { code: "en", name: "English" },
  { code: "de", name: "Deutsch" },
  { code: "es", name: "Español" },
  { code: "fr", name: "Français" },
  { code: "it", name: "Italiano" },
  { code: "ja", name: "日本語" },
  { code: "ko", name: "한국어" },
  { code: "nl", name: "Nederlands" },
  { code: "pl", name: "Polski" },
  { code: "pt-br", name: "Português (Brasil)" },
  { code: "sv", name: "Svenska" },
  { code: "tr", name: "Türkçe" },
  { code: "zh-cn", name: "简体中文" },
  { code: "zh-tw", name: "繁體中文" },
] as const;

export type Locale = (typeof LOCALES)[number]["code"];
/** What the viewer picked: a language, or `system` to follow the browser's languages. */
export type LocalePreference = Locale | "system";

export const SOURCE_LOCALE: Locale = "en";
const STORAGE_KEY = "noalbs-locale";
const listeners = new Set<() => void>();

const isLocale = (code: string): code is Locale => LOCALES.some((locale) => locale.code === code);

/** The stored preference; anything else (nothing saved yet, or unreadable) is `system`. */
export function parsePreference(saved: string | null): LocalePreference {
  return saved !== null && isLocale(saved) ? saved : "system";
}

/**
 * The first of the browser's languages we have: the exact tag ("pt-BR"), else on
 * the language alone ("nl-BE" gets "nl", "pt-PT" gets "pt-br"), else the source language.
 * Chinese goes by script: traditional for Hant, Hong Kong and Macau, else simplified.
 */
export function pickLocale(requested: readonly string[]): Locale {
  for (const tag of requested) {
    const code = tag.toLowerCase();
    if (isLocale(code)) return code;
    if (/^zh-(hant|hk|mo)\b/.test(code)) return "zh-tw";
    const language = code.split("-")[0];
    const match = LOCALES.find((locale) => locale.code.split("-")[0] === language);
    if (match) return match.code;
  }
  return SOURCE_LOCALE;
}

function readPreference(): LocalePreference {
  try {
    return parsePreference(localStorage.getItem(STORAGE_KEY));
  } catch {
    return "system";
  }
}

export function resolveLocale(preference: LocalePreference, requested: readonly string[]): Locale {
  return preference === "system" ? pickLocale(requested) : preference;
}

const browserLanguages = () => (typeof navigator === "undefined" ? [] : navigator.languages);

/** Loads a language's catalog and makes it the active one. */
async function activate(locale: Locale) {
  const { messages } = await import(`../locales/${locale}/messages.po`);
  i18n.load(locale, messages);
  i18n.activate(locale);
  document.documentElement.lang = locale;
  for (const listener of listeners) listener();
}

/** Activates the viewer's language; the app waits for this before its first render. */
export function initLocale(): Promise<void> {
  return activate(resolveLocale(readPreference(), browserLanguages()));
}

export async function setLocalePreference(preference: LocalePreference) {
  try {
    localStorage.setItem(STORAGE_KEY, preference);
  } catch {
    // Storage can be unavailable (private mode); the choice then lasts this visit.
  }
  await activate(resolveLocale(preference, browserLanguages()));
}

/** The language in use, for `Intl` and `toLocale…String`. */
export const currentLocale = (): Locale => (isLocale(i18n.locale) ? i18n.locale : SOURCE_LOCALE);

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};

/** The language in use and the viewer's preference. */
export function useLocale() {
  const locale = useSyncExternalStore(subscribe, currentLocale, () => SOURCE_LOCALE);
  const preference = useSyncExternalStore(subscribe, readPreference, () => "system" as const);
  return { locale, preference, setPreference: setLocalePreference };
}
