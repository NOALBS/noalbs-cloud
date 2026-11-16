import { useState } from "react";
import { Button } from "@/components/ui/button";

export type LiveVariant = "a" | "b" | "c";
export const variantNames = { a: "A · Cards", b: "B · Hairline", c: "C · Console" };
export const families = ["scenes", "actions", "servers", "activity", "monitors"] as const;
export type Family = (typeof families)[number];
export type FamilyVariants = Record<Family, LiveVariant>;
export const paneFamily = (id: string): Family | null =>
  id === "controls"
    ? "actions"
    : id === "program-preview" || id === "preview"
      ? "monitors"
      : families.includes(id as Family)
        ? (id as Family)
        : null;
export function parseVariant(value: string | null): LiveVariant {
  return value === "b" || value === "c" ? value : "a";
}
export function useLiveVariants() {
  const [variant, setVariant] = useState(() =>
    parseVariant(new URLSearchParams(location.search).get("variant")),
  );
  const [mix, setMix] = useState<Partial<FamilyVariants>>({});
  const designs = Object.fromEntries(
    families.map((family) => [family, mix[family] ?? variant]),
  ) as FamilyVariants;
  const choose = (value: LiveVariant) => {
    const url = new URL(location.href);
    url.searchParams.set("variant", value);
    history.replaceState(history.state, "", url);
    setVariant(value);
    setMix({});
  };
  return {
    variant,
    designs,
    choose,
    mix: (family: Family, value: LiveVariant | "") =>
      setMix((current) => ({ ...current, [family]: value || undefined })),
  };
}
const descriptions: Record<Family, string> = {
  scenes: "tiles / rows / bus dock",
  actions: "tiles / compact rows / command list",
  servers: "cards / hairlines / rail",
  activity: "entries / time rows / timeline",
  monitors: "control footer / compact pair / side transition",
};
export function VariantPicker({ model }: { model: ReturnType<typeof useLiveVariants> }) {
  const [open, setOpen] = useState(false);
  if (!import.meta.env.DEV) return null;
  return (
    <aside
      className="live-variant-picker fixed bottom-20 right-4 z-50 rounded-2xl border bg-card p-3 shadow-lg md:bottom-4"
      aria-label="Prototype design picker"
    >
      <Button size="sm" variant="ghost" onClick={() => setOpen(!open)} aria-expanded={open}>
        Prototype only · {variantNames[model.variant]}
      </Button>
      {open && (
        <div className="mt-3 flex max-w-72 flex-col gap-3 text-sm">
          <label className="flex items-center justify-between gap-3">
            Arrangement
            <select
              aria-label="Design variant"
              value={model.variant}
              onChange={(event) => model.choose(parseVariant(event.target.value))}
              className="rounded-md border bg-background p-2"
            >
              {Object.entries(variantNames).map(([id, name]) => (
                <option key={id} value={id}>
                  {name}
                </option>
              ))}
            </select>
          </label>
          <p className="text-xs text-muted-foreground">
            Mix pane families without changing the website theme.
          </p>
          {families.map((family) => (
            <label key={family} className="flex items-center justify-between gap-3">
              <span className="capitalize">
                {family}
                <small className="block text-[11px] normal-case text-muted-foreground">
                  {descriptions[family]}
                </small>
              </span>
              <select
                aria-label={`${family} design`}
                value={model.designs[family]}
                onChange={(event) => model.mix(family, parseVariant(event.target.value))}
                className="rounded-md border bg-background p-2"
              >
                {Object.entries(variantNames).map(([id, name]) => (
                  <option key={id} value={id}>
                    {name}
                  </option>
                ))}
              </select>
            </label>
          ))}
        </div>
      )}
    </aside>
  );
}
