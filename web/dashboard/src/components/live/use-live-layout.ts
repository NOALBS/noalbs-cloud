import { useCallback, useState } from "react";
import {
  ALL_DEVICES,
  type Availability,
  type Columns,
  type DesktopLayout,
  effectiveTiles,
  layoutStorageKey,
  normalize,
  normalizePhone,
  type PhoneLayout,
  type PhoneSection,
  type Preset,
  parseLayout,
  parsePhoneLayout,
  phoneStorageKey,
  serializeLayout,
  serializePhoneLayout,
  type Tile,
} from "./layout";

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Storage can be unavailable (private mode); the choice then lasts this visit.
  }
}
function useStored<T>(
  key: string,
  parse: (raw: string | null) => T,
  serialize: (value: T) => string,
) {
  const [stored, setStored] = useState(() => ({ key, value: parse(read(key)) }));
  let value = stored.value;
  if (stored.key !== key) {
    value = parse(read(key));
    setStored({ key, value });
  }
  const update = useCallback(
    (change: (current: T) => T) => {
      setStored((current) => {
        const next = change(current.key === key ? current.value : parse(read(key)));
        write(key, serialize(next));
        return { key, value: next };
      });
    },
    [key, parse, serialize],
  );
  return [value, update] as const;
}
const readDesktop = (raw: string | null): DesktopLayout =>
  parseLayout(raw, ALL_DEVICES) ?? { v: 1, preset: "default", grids: {} };
const readPhone = (raw: string | null): PhoneLayout =>
  parsePhoneLayout(raw) ?? { v: 1, sections: normalizePhone(null) };

export function useLiveLayout(
  userId: string,
  switcherId: string,
  cols: Columns,
  availability: Availability,
) {
  const [layout, update] = useStored(
    layoutStorageKey(userId, switcherId),
    readDesktop,
    serializeLayout,
  );
  const [phone, updatePhone] = useStored(
    phoneStorageKey(userId, switcherId),
    readPhone,
    serializePhoneLayout,
  );
  const setPreset = (preset: Preset) => update((current) => ({ ...current, preset, grids: {} }));
  const reset = () => update((current) => ({ ...current, grids: {} }));
  const updateTiles = (change: readonly Tile[] | ((tiles: Tile[]) => readonly Tile[])) =>
    update((current) => {
      const tiles = effectiveTiles(current, cols, availability);
      const next = typeof change === "function" ? change(tiles) : change;
      return {
        ...current,
        grids: { ...current.grids, [cols]: normalize(next, cols, availability) },
      };
    });
  const updateSections = (
    change: readonly PhoneSection[] | ((sections: PhoneSection[]) => readonly PhoneSection[]),
  ) =>
    updatePhone((current) => ({
      v: 1,
      sections: normalizePhone(typeof change === "function" ? change(current.sections) : change),
    }));
  return {
    layout,
    preset: layout.preset,
    tiles: effectiveTiles(layout, cols, availability),
    setPreset,
    reset,
    updateTiles,
    sections: phone.sections,
    updateSections,
  };
}
