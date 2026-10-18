import { Trans } from "@lingui/react/macro";
import { Languages } from "lucide-react";
import {
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
} from "@/components/ui/dropdown-menu";
import { LOCALES, type LocalePreference, useLocale } from "@/lib/locale";

/** The language choice for the account menu. Nothing to pick from until there's a second language. */
export function LanguageMenu() {
  const { preference, setPreference } = useLocale();
  if (LOCALES.length < 2) return null;

  return (
    <>
      <DropdownMenuSeparator />
      <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
        <Trans>Language</Trans>
      </DropdownMenuLabel>
      <DropdownMenuRadioGroup
        value={preference}
        onValueChange={(value) => void setPreference(value as LocalePreference)}
      >
        <DropdownMenuRadioItem value="system" onSelect={(event) => event.preventDefault()}>
          <Languages />
          <Trans>Auto</Trans>
        </DropdownMenuRadioItem>
        {LOCALES.map((locale) => (
          <DropdownMenuRadioItem
            key={locale.code}
            value={locale.code}
            lang={locale.code}
            onSelect={(event) => event.preventDefault()}
          >
            {locale.name}
          </DropdownMenuRadioItem>
        ))}
      </DropdownMenuRadioGroup>
    </>
  );
}
