import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Eye, EyeOff, SlidersHorizontal, Volume2, VolumeX } from "lucide-react";
import { type RefObject, useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { obsAudioQuery, queryKeys } from "@/api/queries";
import type { AudioUpdate, ChannelLevel, ObsAudio, ObsAudioInput } from "@/api/types";
import { Skeleton } from "@/components/ui/skeleton";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { useDocumentVisible } from "@/hooks/use-document-visible";
import {
  ERROR_DB,
  faderDb,
  faderPosition,
  formatDb,
  type MeterState,
  meterFraction,
  SCALE_TICKS,
  SILENT_METER,
  stepMeter,
  useMetersSetting,
  WARNING_DB,
  withMuted,
  withVolume,
} from "@/lib/audio";
import { errorMessage } from "@/lib/errors";
import { sceneKey, useHiddenAudio } from "@/lib/hidden-scenes";
import { cn } from "@/lib/utils";

type Path = { workspace_id: string; switcher_id: string };

/** How often the list is re-read, so inputs added in OBS show up. */
const REFRESH_MS = 10_000;
/** Levels older than this are silence (OBS or the feed went away). */
const LEVELS_STALE_MS = 1_000;
/** Until an input has been heard, it's drawn as stereo. */
const DEFAULT_CHANNELS = 2;
/** While dragging a fader, a position goes to OBS at most this often. */
const FADER_SEND_MS = 150;
/** OBS echoes each position back; for this long after a move here, echoes are ignored. */
const FADER_ECHO_MS = 1_000;
/** 100 steps from bottom to top (0.2 dB apart near 0 dB). */
const FADER_STEP = 0.01;

/** Like the Scenes and Sources cards' header buttons. */
const HEADER_BUTTON =
  "flex h-8 items-center gap-1.5 rounded-lg px-2 text-xs font-medium hover:bg-surface-2";

const warning = meterFraction(WARNING_DB) * 100;
const error = meterFraction(ERROR_DB) * 100;
const ZONES = `linear-gradient(to right, var(--live) 0 ${warning}%, var(--low) ${warning}% ${error}%, var(--offline) ${error}% 100%)`;

interface Levels {
  byInput: Map<string, ChannelLevel[]>;
  at: number;
}

/**
 * The switcher's audio feed: levels (when `levels`, which makes OBS send them)
 * into a ref for the meters to draw from, mute and fader changes to `onChange`.
 * Returns the ref and how many channels each input has.
 */
function useAudioFeed(
  path: Path,
  active: boolean,
  levels: boolean,
  onChange: (update: AudioUpdate) => void,
) {
  const levelsRef = useRef<Levels>({ byInput: new Map(), at: 0 });
  const [channels, setChannels] = useState<Record<string, number>>({});
  const onChangeRef = useRef(onChange);
  useEffect(() => {
    onChangeRef.current = onChange;
  });

  useEffect(() => {
    if (!active) return;
    const source = new EventSource(
      `/api/workspaces/${path.workspace_id}/switchers/${path.switcher_id}/obs/audio/live?levels=${levels}`,
      { withCredentials: true },
    );
    source.addEventListener("audio", (message) => {
      const update = JSON.parse(message.data) as AudioUpdate;
      if (update.type !== "levels") {
        onChangeRef.current(update);
        return;
      }
      levelsRef.current = {
        byInput: new Map(update.inputs.map((input) => [input.input, input.channels])),
        at: performance.now(),
      };
      setChannels((current) => {
        const changed = update.inputs.filter(
          (input) => current[input.input] !== input.channels.length,
        );
        if (changed.length === 0) return current;
        const next = { ...current };
        for (const input of changed) next[input.input] = input.channels.length;
        return next;
      });
    });
    return () => {
      source.close();
      levelsRef.current = { byInput: new Map(), at: 0 };
    };
  }, [active, levels, path.workspace_id, path.switcher_id]);

  return { levelsRef, channels };
}

/**
 * Sends fader moves to OBS at most every `FADER_SEND_MS` per input while
 * dragging (the last position always goes), and says whether an input was moved
 * here just now: OBS's echoes of the earlier positions are then ignored, so the
 * slider doesn't jump back under the finger.
 */
function useFaderSender(path: Path) {
  const inputs = useRef(new Map<string, { timer?: number; pending?: number; movedAt: number }>());

  const move = useCallback(
    (input: string, volumeDb: number) => {
      const send = (volume_db: number) =>
        unwrap(
          api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/audio/volume", {
            params: { path: { workspace_id: path.workspace_id, switcher_id: path.switcher_id } },
            body: { input, volume_db },
          }),
        ).catch((error: unknown) => toast.error(errorMessage(error), { id: `volume-${input}` }));

      const state = inputs.current.get(input) ?? { movedAt: 0 };
      inputs.current.set(input, state);
      state.movedAt = performance.now();
      if (state.timer !== undefined) {
        state.pending = volumeDb;
        return;
      }
      void send(volumeDb);
      const flush = () => {
        if (state.pending === undefined) {
          state.timer = undefined;
          return;
        }
        void send(state.pending);
        state.pending = undefined;
        state.timer = window.setTimeout(flush, FADER_SEND_MS);
      };
      state.timer = window.setTimeout(flush, FADER_SEND_MS);
    },
    [path.workspace_id, path.switcher_id],
  );

  const movedRecently = useCallback(
    (input: string) =>
      performance.now() - (inputs.current.get(input)?.movedAt ?? -Infinity) < FADER_ECHO_MS,
    [],
  );

  return { move, movedRecently };
}

interface Meter {
  input: string;
  channel: number;
  el: HTMLDivElement;
  state: MeterState;
}

type Register = (input: string, channel: number, el: HTMLDivElement) => () => void;

/**
 * Draws every registered meter once per animation frame, straight to the DOM
 * (no React renders), falling smoothly between the ~5 updates a second.
 */
function useMeterAnimation(levelsRef: RefObject<Levels>, running: boolean): Register {
  const meters = useRef(new Map<string, Meter>());

  useEffect(() => {
    if (!running) return;
    let frame = 0;
    let last = performance.now();
    const draw = (now: number) => {
      const dt = Math.min(250, now - last);
      last = now;
      const { byInput, at } = levelsRef.current;
      const fresh = now - at < LEVELS_STALE_MS;
      for (const meter of meters.current.values()) {
        const level = fresh ? byInput.get(meter.input)?.[meter.channel] : undefined;
        meter.state = stepMeter(meter.state, level, dt, now);
        paint(meter.el, meter.state);
      }
      frame = requestAnimationFrame(draw);
    };
    frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
  }, [levelsRef, running]);

  return useCallback((input, channel, el) => {
    const key = `${channel}:${input}`;
    meters.current.set(key, { input, channel, el, state: SILENT_METER });
    paint(el, SILENT_METER);
    return () => {
      meters.current.delete(key);
    };
  }, []);
}

function paint(el: HTMLDivElement, state: MeterState) {
  const [, fill, magnitude, hold] = el.children as unknown as [
    HTMLElement,
    HTMLElement,
    HTMLElement,
    HTMLElement,
  ];
  const peak = meterFraction(state.peak) * 100;
  fill.style.clipPath = `inset(0 ${100 - peak}% 0 0)`;
  const place = (marker: HTMLElement, db: number) => {
    const at = meterFraction(db) * 100;
    marker.style.left = `calc(${at}% - 2px)`;
    marker.style.opacity = at > 0 ? "1" : "0";
  };
  place(magnitude, state.magnitude);
  place(hold, state.hold);
}

/**
 * OBS's Audio Mixer for the scene on air: a strip per input with its meters
 * (while levels are on, which has OBS send them), a fader and a mute button. Levels are
 * off until the viewer turns them on (per browser); they stop while the
 * tab is hidden. "Customize" hides inputs from the card for this viewer (on every
 * scene), like the Sources card.
 */
export function AudioMixer({
  path,
  scene,
  canControl,
  obsConnected,
}: {
  path: Path;
  /** The scene on air (UUID, else name), so a switch loads the new scene's inputs. */
  scene: string | null;
  canControl: boolean;
  obsConnected: boolean;
}) {
  const [metersOn, setMetersOn] = useMetersSetting();
  const [hidden, setHidden] = useHiddenAudio(path.switcher_id);
  const [editing, setEditing] = useState(false);
  const visible = useDocumentVisible();
  const queryClient = useQueryClient();
  const query = obsAudioQuery(path.workspace_id, path.switcher_id, scene);
  const list = useQuery({ ...query, enabled: obsConnected, refetchInterval: REFRESH_MS });

  const updateLists = useCallback(
    (change: (data: ObsAudio) => ObsAudio) =>
      queryClient.setQueriesData<ObsAudio>(
        { queryKey: queryKeys.obsAudio(path.workspace_id, path.switcher_id) },
        (data) => data && change(data),
      ),
    [queryClient, path.workspace_id, path.switcher_id],
  );

  const fader = useFaderSender(path);
  const { levelsRef, channels } = useAudioFeed(
    path,
    obsConnected && visible,
    metersOn,
    (update) => {
      if (update.type === "muted")
        updateLists((data) => withMuted(data, update.input, update.muted));
      if (update.type === "volume" && !fader.movedRecently(update.input))
        updateLists((data) => withVolume(data, update.input, update.volume_db));
    },
  );
  const register = useMeterAnimation(levelsRef, metersOn && obsConnected && visible);

  const mute = useMutation({
    mutationFn: ({ input, muted }: { input: string; muted: boolean }) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/obs/audio/mute", {
          params: { path },
          body: { input, muted },
        }),
      ),
    onMutate: async ({ input, muted }) => {
      await queryClient.cancelQueries({ queryKey: query.queryKey });
      const previous = queryClient.getQueryData(query.queryKey);
      updateLists((data) => withMuted(data, input, muted));
      return { previous };
    },
    onError: (error, _, context) => {
      if (context?.previous) queryClient.setQueryData(query.queryKey, context.previous);
      toast.error(errorMessage(error));
    },
    onSettled: () =>
      queryClient.invalidateQueries({
        queryKey: queryKeys.obsAudio(path.workspace_id, path.switcher_id),
      }),
  });

  const inputs = list.data?.inputs ?? [];
  const hiddenSet = new Set(hidden);
  const listed = inputs.filter((input) => !hiddenSet.has(sceneKey(input)));
  const onScene = inputs.map((input) => sceneKey(input));
  const hiddenHere = inputs.length - listed.length;

  const toggleHidden = (key: string) =>
    setHidden(hiddenSet.has(key) ? hidden.filter((item) => item !== key) : [...hidden, key]);
  // Only this scene's inputs: what was hidden on other scenes stays hidden.
  const setAllHere = (hide: boolean) =>
    setHidden(
      hide ? [...new Set([...hidden, ...onScene])] : hidden.filter((key) => !onScene.includes(key)),
    );

  return (
    <section aria-label="Audio" className="flex flex-col gap-2.5">
      <div className="flex min-h-8 items-center justify-between gap-2">
        <h2 className="font-semibold">Audio</h2>
        {obsConnected &&
          (editing ? (
            <span className="flex items-center gap-1">
              <button
                type="button"
                onClick={() => setAllHere(hiddenHere === 0)}
                className={cn(HEADER_BUTTON, "text-muted-foreground hover:text-foreground")}
              >
                {hiddenHere > 0 ? "Show all" : "Hide all"}
              </button>
              <button
                type="button"
                onClick={() => setEditing(false)}
                className={cn(HEADER_BUTTON, "bg-primary text-primary-foreground hover:bg-primary")}
              >
                Done
              </button>
            </span>
          ) : (
            <span className="flex items-center gap-1 text-xs text-muted-foreground">
              {hiddenHere > 0 && <span className="mr-1">{hiddenHere} hidden</span>}
              <span className="flex items-center gap-2">
                Levels
                <Switch
                  checked={metersOn}
                  onCheckedChange={setMetersOn}
                  aria-label="Show audio levels"
                />
              </span>
              {inputs.length > 0 && (
                <button
                  type="button"
                  onClick={() => setEditing(true)}
                  className={cn(HEADER_BUTTON, "ml-1 text-primary")}
                >
                  <SlidersHorizontal className="size-3.5" />
                  Customize
                </button>
              )}
            </span>
          ))}
      </div>

      {editing && (
        <p className="text-xs text-muted-foreground">
          Tap inputs to show or hide them here. Only this browser remembers it, and OBS isn’t
          changed.
        </p>
      )}

      {!obsConnected && (
        <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
          Audio appears once OBS is connected.
        </p>
      )}
      {obsConnected && list.isPending && <Skeleton className="h-28 rounded-2xl" />}
      {obsConnected && list.isError && !list.data && (
        <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
          {errorMessage(list.error)}
        </p>
      )}
      {list.data && inputs.length === 0 && (
        <p className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
          Nothing on this scene has audio.
        </p>
      )}

      {editing && inputs.length > 0 && (
        <div className="flex flex-col divide-y divide-divider rounded-2xl border bg-card">
          {inputs.map((input) => {
            const key = sceneKey(input);
            const shown = !hiddenSet.has(key);
            return (
              <button
                key={input.name}
                type="button"
                aria-pressed={shown}
                onClick={() => toggleHidden(key)}
                className={cn(
                  "flex items-center gap-3 px-3 py-2.5 text-left transition-colors hover:bg-surface-2",
                  !shown && "text-muted-foreground",
                )}
              >
                <span className="flex size-9 shrink-0 items-center justify-center">
                  {shown ? (
                    <Eye className="size-4.5 text-primary" aria-hidden="true" />
                  ) : (
                    <EyeOff className="size-4.5" aria-hidden="true" />
                  )}
                </span>
                <span className={cn("truncate text-sm font-semibold", !shown && "opacity-70")}>
                  {input.name}
                </span>
                {input.global && <GlobalTag />}
                <span
                  className={cn(
                    "ml-auto shrink-0 text-[11px] font-semibold tracking-wider",
                    shown && "text-primary-soft-foreground",
                  )}
                >
                  {shown ? "SHOWN" : "HIDDEN"}
                </span>
              </button>
            );
          })}
        </div>
      )}

      {!editing && inputs.length > 0 && listed.length === 0 && (
        <button
          type="button"
          onClick={() => setEditing(true)}
          className="rounded-2xl border border-dashed px-4 py-6 text-sm text-muted-foreground hover:bg-surface-2"
        >
          All audio inputs are hidden. Tap to pick some.
        </button>
      )}

      {!editing && listed.length > 0 && (
        <div className="flex flex-col divide-y divide-divider rounded-2xl border bg-card">
          {listed.map((input) => (
            <AudioStrip
              key={input.name}
              input={input}
              channels={channels[input.name] ?? DEFAULT_CHANNELS}
              meters={metersOn}
              register={register}
              canControl={canControl}
              muting={mute.isPending && mute.variables.input === input.name}
              onMute={(muted) => mute.mutate({ input: input.name, muted })}
              onVolume={(volumeDb) => {
                updateLists((data) => withVolume(data, input.name, volumeDb));
                fader.move(input.name, volumeDb);
              }}
            />
          ))}
          {metersOn && <Scale />}
        </div>
      )}
    </section>
  );
}

function AudioStrip({
  input,
  channels,
  meters,
  register,
  canControl,
  muting,
  onMute,
  onVolume,
}: {
  input: ObsAudioInput;
  channels: number;
  meters: boolean;
  register: Register;
  canControl: boolean;
  /** A mute or unmute of this input is on its way. */
  muting: boolean;
  onMute: (muted: boolean) => void;
  onVolume: (volumeDb: number) => void;
}) {
  const Icon = input.muted ? VolumeX : Volume2;
  return (
    <div className="flex items-center gap-3 px-3 py-2.5">
      <button
        type="button"
        aria-pressed={input.muted}
        aria-label={`${input.muted ? "Unmute" : "Mute"} ${input.name}`}
        title={input.muted ? "Unmute" : "Mute"}
        disabled={!canControl || muting}
        onClick={() => onMute(!input.muted)}
        className={cn(
          "flex size-9 shrink-0 items-center justify-center rounded-lg transition-colors disabled:cursor-default",
          input.muted
            ? "bg-offline/15 text-offline"
            : "text-muted-foreground enabled:hover:bg-surface-2 enabled:hover:text-foreground",
        )}
      >
        <Icon className="size-4.5" aria-hidden="true" />
      </button>
      <div className="flex min-w-0 flex-1 flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <span className="truncate text-sm font-semibold">{input.name}</span>
          {input.global && <GlobalTag />}
          <span className="ml-auto shrink-0 font-mono text-xs text-muted-foreground tabular">
            {formatDb(input.volume_db)}
          </span>
        </div>
        <Slider
          min={0}
          max={1}
          step={FADER_STEP}
          value={[faderPosition(input.volume_db)]}
          onValueChange={([position]) => {
            if (position !== undefined) onVolume(faderDb(position));
          }}
          disabled={!canControl}
          aria-label={`Volume of ${input.name}`}
          aria-valuetext={formatDb(input.volume_db)}
          className="py-1"
        />
        {meters && (
          <div className={cn("flex flex-col gap-0.5", input.muted && "grayscale")}>
            {Array.from({ length: channels }, (_, channel) => (
              <ChannelMeter
                // biome-ignore lint/suspicious/noArrayIndexKey: channels are positions
                key={channel}
                input={input.name}
                channel={channel}
                register={register}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function GlobalTag() {
  return (
    <span className="rounded bg-primary-soft px-1.5 py-px text-[10px] font-semibold tracking-wider text-primary-soft-foreground uppercase">
      Global
    </span>
  );
}

function ChannelMeter({
  input,
  channel,
  register,
}: {
  input: string;
  channel: number;
  register: Register;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (ref.current) return register(input, channel, ref.current);
  }, [register, input, channel]);

  return (
    // Painted by `paint`, in this order: unlit scale, lit bar, magnitude tick, peak marker.
    <div ref={ref} className="relative h-1.5 overflow-hidden rounded-[2px]" aria-hidden="true">
      <div className="absolute inset-0 opacity-20" style={{ backgroundImage: ZONES }} />
      <div className="absolute inset-0" style={{ backgroundImage: ZONES }} />
      <div className="absolute inset-y-0 w-0.5 bg-black/45" />
      <div className="absolute inset-y-0 w-0.5 bg-foreground" />
    </div>
  );
}

/** -60 … 0 dB under the meters, lined up with them. */
function Scale() {
  return (
    <div className="flex gap-3 px-3 pt-1 pb-2" aria-hidden="true">
      <span className="w-9 shrink-0" />
      <div className="relative h-3.5 flex-1 font-mono text-[10px] text-muted-foreground">
        {SCALE_TICKS.map((db) => (
          <span
            key={db}
            className={cn(
              "absolute top-0",
              db === 0 ? "-translate-x-full" : db > -60 && "-translate-x-1/2",
            )}
            style={{ left: `${meterFraction(db) * 100}%` }}
          >
            {db}
          </span>
        ))}
      </div>
    </div>
  );
}
