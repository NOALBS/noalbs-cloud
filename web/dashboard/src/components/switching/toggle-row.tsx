import type { ComponentProps } from "react";
import { ToggleRow as SettingToggleRow } from "@/components/settings";
export function ToggleRow({ note, ...props }: ComponentProps<typeof SettingToggleRow>) {
  return (
    <>
      <SettingToggleRow {...props} />
      {note && <p className="px-5 pb-3 text-xs text-muted-foreground">{note}</p>}
    </>
  );
}
