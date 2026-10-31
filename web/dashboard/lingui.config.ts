import { defineConfig } from "@lingui/cli";
import { formatter } from "@lingui/format-po";

// To add a language: add it here and to `LOCALES` in src/lib/locale.ts, then
// `pnpm i18n:extract` and translate src/locales/<locale>/messages.po.
export default defineConfig({
  sourceLocale: "en",
  locales: [
    "en",
    "de",
    "es",
    "fr",
    "it",
    "ja",
    "ko",
    "nl",
    "pl",
    "pt-br",
    "sv",
    "tr",
    "zh-cn",
    "zh-tw",
  ],
  catalogs: [{ path: "<rootDir>/src/locales/{locale}/messages", include: ["src"] }],
  format: formatter({ lineNumbers: false }),
});
