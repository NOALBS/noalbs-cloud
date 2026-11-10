import { t } from "@lingui/core/macro";
import { Section } from "@/components/page";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { LOCALES, type LocalePreference, useLocale } from "@/lib/locale";
import { useTextSize } from "@/lib/text-size";
import { useTheme } from "@/lib/theme";
export function DisplaySettings() {
  const theme = useTheme(),
    text = useTextSize(),
    locale = useLocale();
  return (
    <Section title={t`Display`} description={t`Just for you, on this browser.`}>
      <div className="flex flex-wrap items-center justify-between gap-3 border-t border-divider px-5 py-4">
        <span className="text-sm font-semibold">{t`Theme`}</span>
        <div className="flex gap-1">
          {(
            [
              ["system", t`System`],
              ["dark", t`Dark`],
              ["light", t`Light`],
            ] as const
          ).map(([value, label]) => (
            <Button
              key={value}
              size="sm"
              variant={theme.preference === value ? "secondary" : "ghost"}
              aria-pressed={theme.preference === value}
              onClick={() => theme.setPreference(value)}
            >
              {label}
            </Button>
          ))}
        </div>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-3 border-t border-divider px-5 py-4">
        <div>
          <p className="text-sm font-semibold">{t`Text size`}</p>
          <p className="text-sm text-muted-foreground">{t`Larger sizes suit 4K monitors at 100% scaling.`}</p>
        </div>
        <div className="flex gap-1">
          {(
            [
              ["default", t`Default`],
              ["large", t`Large`],
              ["larger", t`Larger`],
            ] as const
          ).map(([value, label]) => (
            <Button
              key={value}
              size="sm"
              variant={text.size === value ? "secondary" : "ghost"}
              aria-pressed={text.size === value}
              onClick={() => text.setSize(value)}
            >
              {label}
            </Button>
          ))}
        </div>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-3 border-t border-divider px-5 py-4">
        <div>
          <p className="text-sm font-semibold">{t`Language`}</p>
          <p className="text-sm text-muted-foreground">
            {locale.preference === "system"
              ? t`Follow your browser’s language.`
              : t`Use the selected language for the dashboard.`}
          </p>
        </div>
        <Select
          value={locale.preference}
          onValueChange={(value) => void locale.setPreference(value as LocalePreference)}
        >
          <SelectTrigger aria-label={t`Language`} className="w-44">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="system">{t`Auto`}</SelectItem>
            {LOCALES.map((item) => (
              <SelectItem key={item.code} value={item.code}>
                {item.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
    </Section>
  );
}
