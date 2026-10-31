import { defineConfig } from "@lingui/cli";
import { formatter } from "@lingui/format-po";

// To add a language: add it here and to `LOCALES` in src/lib/locale.ts, then
// `pnpm i18n:extract` and translate src/locales/<locale>/messages.po.
export default defineConfig({
  sourceLocale: "en",
  locales: [
    "en",
    "da",
    "de",
    "es",
    "fr",
    "it",
    "ja",
    "ko",
    "nb",
    "nl",
    "pl",
    "pt-br",
    "ru",
    "sv",
    "tr",
    "uk",
    "zh-cn",
    "zh-tw",
  ],
  catalogs: [{ path: "<rootDir>/src/locales/{locale}/messages", include: ["src"] }],
  format: formatter({ lineNumbers: false }),
});
