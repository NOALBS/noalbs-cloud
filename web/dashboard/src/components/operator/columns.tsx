import { plural, t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { Button } from "@/components/ui/button";
import { CompactLive } from "./compact-live";
import type { OperatorItem } from "./model";
import { fillPins, type OperatorPin, pinOf } from "./pins";

export function OperatorColumns({
  items,
  pins,
  onChange,
}: {
  items: OperatorItem[];
  pins: OperatorPin[];
  onChange: (pins: OperatorPin[]) => void;
}) {
  return (
    <section aria-label={t`Side by side`} className="flex min-w-0 flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-xs text-muted-foreground">
          {plural(pins.length, {
            one: "# of 4 columns · the layout is saved for you",
            other: "# of 4 columns · the layout is saved for you",
          })}
        </p>
        <div className="flex flex-wrap gap-2">
          <select
            aria-label={t`Add a switcher`}
            value=""
            disabled={pins.length >= 4}
            className="h-9 max-w-full rounded-lg border bg-card px-3 text-xs"
            onChange={(event) => {
              const item = items.find((item) => item.switcher_id === event.target.value);
              if (item) onChange([...pins, pinOf(item)]);
            }}
          >
            <option value="">
              <Trans>Add a switcher</Trans>
            </option>
            {items
              .filter((item) => !pins.some((pin) => pin.switcher === item.switcher_id))
              .map((item) => (
                <option key={item.switcher_id} value={item.switcher_id}>
                  {item.workspace_name} / {item.switcher_name}
                </option>
              ))}
          </select>
          <Button
            variant="secondary"
            size="sm"
            disabled={
              pins.length >= 4 ||
              !items.some(
                (item) => item.connected && !pins.some((pin) => pin.switcher === item.switcher_id),
              )
            }
            onClick={() => onChange(fillPins(pins, items))}
          >
            <Trans>Fill with what needs attention</Trans>
          </Button>
        </div>
      </div>
      {pins.length === 0 ? (
        <div className="rounded-xl border border-dashed px-4 py-10 text-center">
          <h3 className="font-display font-bold">
            <Trans>Keep up to four switchers side by side</Trans>
          </h3>
          <p className="mt-2 text-sm text-muted-foreground">
            <Trans>
              Add a switcher here, or pin one from its live panel. Your columns follow you between
              workspaces.
            </Trans>
          </p>
        </div>
      ) : (
        <div className="overflow-x-auto pb-2">
          <div className="grid auto-cols-[minmax(340px,1fr)] grid-flow-col items-start gap-3">
            {pins.map((pin) => (
              <CompactLive
                key={`${pin.workspace}/${pin.switcher}`}
                pin={pin}
                column
                pinned
                canPin={false}
                onPin={() => {}}
                onClose={() => onChange(pins.filter((item) => item.switcher !== pin.switcher))}
              />
            ))}
          </div>
        </div>
      )}
    </section>
  );
}
