import { Trans } from "@lingui/react/macro";
import { Languages } from "lucide-react";
import {
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
} from "@/components/ui/dropdown-menu";
import { LOCALES, type LocalePreference, useLocale } from "@/lib/locale";

/** The language choice for the account menu. */
export function LanguageMenu() {
  const { preference, setPreference } = useLocale();

  return (
    <>
      <DropdownMenuSeparator />
      <DropdownMenuSub>
        <DropdownMenuSubTrigger>
          <Languages />
          <Trans>Language</Trans>
        </DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <DropdownMenuRadioGroup
            value={preference}
            onValueChange={(value) => void setPreference(value as LocalePreference)}
          >
            <DropdownMenuRadioItem value="system" onSelect={(event) => event.preventDefault()}>
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
        </DropdownMenuSubContent>
      </DropdownMenuSub>
    </>
  );
}
