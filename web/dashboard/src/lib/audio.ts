import { useCallback, useState } from "react";
import type { ChannelLevel, ObsAudio } from "@/api/types";

/**
 * The Live page's audio mixer, drawn like OBS's: a meter from -60 to 0 dBFS
 * per channel, green up to -20, yellow up to -9, red above. The bar shows the
 * peak, a tick the average loudness, and a marker the recent highest peak.
 * Levels arrive about 10 times a second; between them the bars fall like OBS's
 * ("fast" decay) so they move smoothly.
 */
export const METER_MIN_DB = -60;
export const WARNING_DB = -20;
export const ERROR_DB = -9;
export const SCALE_TICKS = [-60, -50, -40, -30, -20, -10, 0] as const;

/** OBS's fast decay rate, in dB per second. */
const DECAY_DB_PER_S = 23.53;
/** How long the peak marker stays before falling. */
const HOLD_MS = 1500;
const SILENCE_DB = -100;

/** Where a level sits on the meter, 0 (≤ -60 dB) to 1 (0 dB). */
export function meterFraction(db: number): number {
  return Math.min(1, Math.max(0, (db - METER_MIN_DB) / -METER_MIN_DB));
}

/** "-3.5 dB", or "-inf dB" with the fader all the way down, like OBS. */
export function formatDb(db: number): string {
  return db <= SILENCE_DB ? "-inf dB" : `${db.toFixed(1)} dB`;
}

/** What one channel's meter shows. */
export interface MeterState {
  peak: number;
  magnitude: number;
  hold: number;
  holdUntil: number;
}

export const SILENT_METER: MeterState = {
  peak: SILENCE_DB,
  magnitude: SILENCE_DB,
  hold: SILENCE_DB,
  holdUntil: 0,
};

/**
 * One animation frame: the meter jumps up to the latest level (`undefined` is
 * silence) and falls towards it at OBS's decay rate. `dt` and `now` in ms.
 */
export function stepMeter(
  state: MeterState,
  level: ChannelLevel | undefined,
  dt: number,
  now: number,
): MeterState {
  const fall = (DECAY_DB_PER_S * dt) / 1000;
  const peak = Math.max(level?.peak ?? SILENCE_DB, state.peak - fall);
  const magnitude = Math.max(level?.magnitude ?? SILENCE_DB, state.magnitude - fall);
  if (peak >= state.hold) return { peak, magnitude, hold: peak, holdUntil: now + HOLD_MS };
  const hold = now < state.holdUntil ? state.hold : Math.max(peak, state.hold - fall);
  return { peak, magnitude, hold, holdUntil: state.holdUntil };
}

/**
 * OBS's fader (libobs `OBS_FADER_LOG`): a position from 0 (bottom, -inf) to 1
 * (0 dB), spread so the useful range gets most of the travel. -96 dB and below
 * sit at the bottom; above 0 dB (only settable through obs-websocket) at the top.
 */
const LOG_OFFSET_DB = 6;
const LOG_RANGE_DB = 96;

export function faderPosition(db: number): number {
  if (db >= 0) return 1;
  if (db <= -LOG_RANGE_DB) return 0;
  const top = Math.log10(LOG_RANGE_DB + LOG_OFFSET_DB);
  return (top - Math.log10(-db + LOG_OFFSET_DB)) / (top - Math.log10(LOG_OFFSET_DB));
}

/** The dB for a fader position, -100 (-inf) at the bottom, rounded to 0.1 dB. */
export function faderDb(position: number): number {
  if (position >= 1) return 0;
  if (position <= 0) return SILENCE_DB;
  const range = LOG_RANGE_DB + LOG_OFFSET_DB;
  const db = -range * (range / LOG_OFFSET_DB) ** -position + LOG_OFFSET_DB;
  return Math.round(db * 10) / 10 || 0;
}

/** The list with one input's mute state changed. */
export function withMuted(list: ObsAudio, input: string, muted: boolean): ObsAudio {
  return {
    ...list,
    inputs: list.inputs.map((item) => (item.name === input ? { ...item, muted } : item)),
  };
}

/** The list with one input's fader changed. */
export function withVolume(list: ObsAudio, input: string, volumeDb: number): ObsAudio {
  return {
    ...list,
    inputs: list.inputs.map((item) =>
      item.name === input ? { ...item, volume_db: volumeDb } : item,
    ),
  };
}

const KEY = "noalbs-audio-meters";

function read(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

/**
 * The viewer's choice to show levels, kept in this browser. Off until turned on:
 * every level update costs the streamer's upload and the viewer's data.
 */
export function useMetersSetting() {
  const [stored, setStored] = useState(read);
  const set = useCallback((on: boolean) => {
    const value = on ? "on" : "off";
    try {
      localStorage.setItem(KEY, value);
    } catch {
      // Storage can be unavailable (private mode); the choice then lasts this visit.
    }
    setStored(value);
  }, []);
  return [stored === "on", set] as const;
}
