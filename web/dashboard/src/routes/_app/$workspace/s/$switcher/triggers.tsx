import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { obsScenesQuery, queryKeys } from "@/api/queries";
import type { Behaviour, Switcher, Triggers } from "@/api/types";
import { Section } from "@/components/page";
import { ReadOnlyNotice, SaveBar, ToggleRow } from "@/components/settings";
import { SettingsHub } from "@/components/settings-hub";
import { SwitcherPage } from "@/components/switcher-page";
import { TriggerField } from "@/components/trigger-field";
import { TriggerScale } from "@/components/trigger-scale";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { useSwitcher } from "@/hooks/use-switcher";
import { errorMessage } from "@/lib/errors";
import { SCENE_ROLES } from "@/lib/scene-roles";
import { bitrateRules, bitrateZones, rttZones, triggerError } from "@/lib/triggers";

export const Route = createFileRoute("/_app/$workspace/s/$switcher/triggers")({
  component: TriggersPage,
});

type TimeoutKey = "offline_timeout" | "ending_timeout";
type BehaviourKey = Exclude<keyof Behaviour, "retry_attempts" | TimeoutKey | "recording_scene_ids">;

/** Minutes, when a "Stop …" timeout is turned on. */
const TIMEOUT_FALLBACK: Record<TimeoutKey, number> = { offline_timeout: 10, ending_timeout: 5 };

const TIMEOUT_NAMES: Record<TimeoutKey, string> = {
  offline_timeout: "offline timeout",
  ending_timeout: "ending timeout",
};

const validTimeout = (minutes: number | null) =>
  minutes === null || (Number.isInteger(minutes) && minutes >= 1 && minutes <= 1440);

const SWITCHING: {
  key: BehaviourKey;
  title: string;
  description: string;
  note?: string;
  /** Only means something while this other setting is on. */
  requires?: BehaviourKey;
}[] = [
  {
    key: "bitrate_switcher_enabled",
    title: "Auto switch",
    description: "Switch scenes automatically using the triggers.",
  },
  {
    key: "only_switch_when_streaming",
    title: "Only while streaming",
    description: "Leave OBS alone when you aren’t live.",
  },
  {
    key: "instantly_switch_on_recover",
    title: "Instant recovery",
    description: "Go back to LIVE the moment the feed recovers.",
  },
  {
    key: "auto_switch_notification",
    title: "Chat notification",
    description:
      "Post in chat when a scene switches automatically. Pick the channels on the Chat page.",
    note: "Needs chat bot",
  },
];

const START_END: typeof SWITCHING = [
  {
    key: "switch_to_starting",
    title: "Starting scene on go-live",
    description: "Show the Starting scene when the stream starts.",
  },
  {
    key: "switch_to_live",
    title: "Then switch to LIVE",
    description: "Leave the Starting scene as soon as your feed arrives.",
  },
  {
    key: "record_while_streaming",
    title: "Record while streaming",
    description: "Start and stop recording together with the stream.",
  },
  {
    key: "auto_stop_on_raid",
    title: "Stop stream after a raid",
    description: "End the stream once you raid out (Twitch and Kick).",
    note: "Needs chat bot",
  },
  {
    key: "announce_raid",
    title: "Announce raids",
    description: "Say in chat when the stream stops for a raid (Twitch and Kick).",
    note: "Needs chat bot",
    requires: "auto_stop_on_raid",
  },
];

interface Form {
  triggers: Triggers;
  behaviour: Behaviour;
}

const formOf = (switcher: Switcher): Form => ({
  triggers: switcher.triggers,
  behaviour: switcher.behaviour,
});

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

function TriggersPage() {
  const { workspace, switcher, canManage, path } = useSwitcher();
  const queryClient = useQueryClient();
  const obs = useQuery(obsScenesQuery(workspace.id, switcher.id));
  const saved = formOf(switcher);
  const [form, setForm] = useState<Form>(saved);
  const [base, setBase] = useState(saved);

  // Adopt fresh server data (e.g. saved in another tab) while nothing is edited here.
  if (!same(saved, base) && same(form, base)) {
    setBase(saved);
    setForm(saved);
  }

  const { triggers, behaviour } = form;
  const setTriggers = (patch: Partial<Triggers>) =>
    setForm((current) => ({ ...current, triggers: { ...current.triggers, ...patch } }));
  const setBehaviour = (patch: Partial<Behaviour>) =>
    setForm((current) => ({ ...current, behaviour: { ...current.behaviour, ...patch } }));

  const badTimeout = (["offline_timeout", "ending_timeout"] as const).find(
    (key) => !validTimeout(behaviour[key] ?? null),
  );
  const problem =
    triggerError(triggers) ??
    (Number.isInteger(behaviour.retry_attempts) &&
    behaviour.retry_attempts >= 0 &&
    behaviour.retry_attempts <= 100
      ? null
      : "Retry attempts must be between 0 and 100.") ??
    (badTimeout ? `The ${TIMEOUT_NAMES[badTimeout]} must be between 1 and 1440 minutes.` : null);

  const save = useMutation({
    mutationFn: async () => {
      let result: Switcher | undefined;
      if (!same(triggers, base.triggers)) {
        result = await unwrap(
          api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/triggers", {
            params: { path },
            body: triggers,
          }),
        );
      }
      if (!same(behaviour, base.behaviour)) {
        result = await unwrap(
          api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/behaviour", {
            params: { path },
            body: behaviour,
          }),
        );
      }
      return result;
    },
    onSuccess: (result) => {
      if (result) {
        queryClient.setQueryData(queryKeys.switcher(workspace.id, switcher.id), result);
        setBase(formOf(result));
        setForm(formOf(result));
      }
      toast.success("Saved");
    },
  });

  const editable = canManage;

  return (
    <SwitcherPage
      title={
        <>
          <span className="md:hidden">Settings</span>
          <span className="hidden md:inline">Switching</span>
        </>
      }
      description={`When and how NOALBS switches scenes for ${switcher.name}.`}
      className="max-w-5xl"
    >
      <SettingsHub />
      {!canManage && <ReadOnlyNotice />}

      <div className="grid grid-cols-1 items-start gap-4 lg:grid-cols-12">
        <div className="flex flex-col gap-4 lg:col-span-7">
          <Section
            title="Bitrate triggers"
            description="Measured on the highest-priority server that is receiving video."
          >
            <div className="flex flex-col gap-5 px-5 pb-5">
              <TriggerScale
                zones={bitrateZones(triggers)}
                ticks={["0", "2,000", "4,000", "6,000", "8,000+ kbps"]}
                label={`Bitrate scale: LOW at ${triggers.low ?? "off"}, BRB at ${triggers.offline ?? "off"} kbps`}
              />
              <div className="grid gap-4 sm:grid-cols-2">
                <TriggerField
                  id="trigger-low"
                  label="Switch to LOW at or below"
                  sceneRole="low"
                  unit="kbps"
                  value={triggers.low ?? null}
                  fallback={800}
                  onChange={(low) => setTriggers({ low })}
                  disabled={!editable}
                />
                <TriggerField
                  id="trigger-offline"
                  label="Switch to BRB at or below"
                  sceneRole="offline"
                  unit="kbps"
                  value={triggers.offline ?? null}
                  fallback={Math.min(150, Math.max(1, (triggers.low ?? 800) - 1))}
                  onChange={(offline) => setTriggers({ offline })}
                  disabled={!editable}
                />
              </div>
              <ul className="grid gap-2.5 sm:grid-cols-3">
                {bitrateRules(triggers).map((rule) => (
                  <li
                    key={rule.role}
                    className="flex flex-col gap-1 rounded-xl border border-divider bg-sunken p-3"
                  >
                    <span className="flex items-center gap-2 text-sm font-bold">
                      <span className={`size-2 rounded-full ${SCENE_ROLES[rule.role].dot}`} />
                      {SCENE_ROLES[rule.role].label}
                    </span>
                    <span className="text-xs text-muted-foreground">{rule.range}</span>
                  </li>
                ))}
              </ul>
            </div>
          </Section>

          <Section
            title="Latency (RTT) triggers"
            description="Catch a struggling uplink before the bitrate drops. Only SRT servers report RTT."
          >
            <div className="flex flex-col gap-5 px-5 pb-5">
              <TriggerScale
                zones={rttZones(triggers)}
                ticks={["0", "1,250", "2,500", "3,750", "5,000+ ms"]}
                label={`RTT scale: LOW at ${triggers.rtt ?? "off"}, BRB at ${triggers.rtt_offline ?? "off"} ms`}
              />
              <div className="grid gap-4 sm:grid-cols-2">
                <TriggerField
                  id="trigger-rtt"
                  label="Switch to LOW at or above"
                  sceneRole="low"
                  unit="ms"
                  value={triggers.rtt ?? null}
                  fallback={2500}
                  onChange={(rtt) => setTriggers({ rtt })}
                  disabled={!editable}
                />
                <TriggerField
                  id="trigger-rtt-offline"
                  label="Switch to BRB at or above"
                  sceneRole="offline"
                  unit="ms"
                  value={triggers.rtt_offline ?? null}
                  fallback={Math.max(5000, (triggers.rtt ?? 2500) + 1)}
                  onChange={(rtt_offline) => setTriggers({ rtt_offline })}
                  disabled={!editable}
                />
              </div>
            </div>
          </Section>
        </div>

        <div className="flex flex-col gap-4 lg:col-span-5">
          <Section title="Switching">
            {SWITCHING.map((row) => (
              <ToggleRow
                key={row.key}
                id={`behaviour-${row.key}`}
                title={row.title}
                description={row.description}
                note={row.note}
                checked={behaviour[row.key]}
                onCheckedChange={(checked) => setBehaviour({ [row.key]: checked })}
                disabled={!editable}
              />
            ))}
            <label
              htmlFor="retry-attempts"
              className="flex min-h-16 items-center gap-4 border-t border-divider px-5 py-3"
            >
              <span className="flex flex-1 flex-col gap-0.5">
                <span className="text-sm font-semibold">Checks before switching</span>
                <span className="text-sm text-muted-foreground">
                  Bad checks in a row before leaving LIVE; one check per second.
                </span>
              </span>
              <Input
                id="retry-attempts"
                type="number"
                min={0}
                max={100}
                value={behaviour.retry_attempts}
                onChange={(event) => setBehaviour({ retry_attempts: Number(event.target.value) })}
                disabled={!editable}
                className="w-20 font-mono tabular"
              />
            </label>
          </Section>

          <Section title="Start, end & raids">
            {START_END.map((row) => (
              <ToggleRow
                key={row.key}
                id={`behaviour-${row.key}`}
                title={row.title}
                description={row.description}
                note={row.note}
                checked={behaviour[row.key]}
                onCheckedChange={(checked) => setBehaviour({ [row.key]: checked })}
                disabled={!editable || (row.requires != null && !behaviour[row.requires])}
              />
            ))}
            <RecordingScenesSetting
              ids={behaviour.recording_scene_ids}
              scenes={obs.data?.scenes}
              obsAvailable={obs.isSuccess}
              onChange={(recording_scene_ids) => setBehaviour({ recording_scene_ids })}
              disabled={!editable || !obs.isSuccess}
            />
            <TimeoutSetting
              setting="offline_timeout"
              title="Stop when offline"
              description="End the stream once the BRB scene has been on air this long without a feed, and say so in chat."
              minutesTitle="Minutes offline"
              minutesDescription="Counts while auto switch is on and BRB is on air; the feed coming back starts it over."
              behaviour={behaviour}
              saved={base.behaviour}
              setBehaviour={setBehaviour}
              disabled={!editable}
            />
            <TimeoutSetting
              setting="ending_timeout"
              title="Stop on the Ending scene"
              description="End the stream once the Ending scene has been on air this long, and say so in chat."
              note={switcher.scenes.ending ? undefined : "Needs Ending scene"}
              minutesTitle="Minutes on Ending"
              minutesDescription="Counts while streaming with the Ending scene on air, even with auto switch off; leaving it starts it over."
              behaviour={behaviour}
              saved={base.behaviour}
              setBehaviour={setBehaviour}
              disabled={!editable}
            />
          </Section>
        </div>
      </div>

      <SaveBar
        dirty={!same(form, base)}
        saving={save.isPending}
        error={problem ?? (save.error ? errorMessage(save.error) : null)}
        disabled={problem !== null}
        onSave={() => save.mutate()}
        onDiscard={() => {
          setForm(base);
          save.reset();
        }}
      />
    </SwitcherPage>
  );
}

/** Select the program scenes on which an active OBS recording is retained. */
function RecordingScenesSetting({
  ids,
  scenes,
  obsAvailable,
  onChange,
  disabled,
}: {
  ids: string[];
  scenes: { uuid?: string | null; name: string }[] | undefined;
  obsAvailable: boolean;
  onChange: (ids: string[]) => void;
  disabled: boolean;
}) {
  const selectable =
    scenes?.filter((scene): scene is { uuid: string; name: string } => !!scene.uuid) ?? [];
  const selected = ids.filter((id, index) => ids.indexOf(id) === index);
  const availableIds = new Set(selectable.map((scene) => scene.uuid));
  const unavailable = selected.filter((id) => !availableIds.has(id));
  const availableSelectedNames = selected
    .filter((id) => !unavailable.includes(id))
    .map((id) => selectable.find((scene) => scene.uuid === id)?.name ?? id);
  const summary =
    availableSelectedNames.length > 0
      ? availableSelectedNames.length <= 2
        ? availableSelectedNames.join(", ")
        : `${availableSelectedNames.slice(0, 2).join(", ")} +${availableSelectedNames.length - 2} more`
      : unavailable.length > 0
        ? `${unavailable.length} missing`
        : "Off";
  const clearUnavailable = () => onChange(selected.filter((id) => !unavailable.includes(id)));

  return (
    <div className="border-t border-divider px-5 py-4">
      <div className="flex flex-col gap-0.5">
        <span className="text-sm font-semibold">Only record on these scenes</span>
        <span className="text-sm text-muted-foreground">
          Pick scenes that keep recording on. Leave this empty to turn it off; recording pauses on
          other scenes and resumes when you return. If you resume it yourself on another scene, it
          keeps recording until the next scene change.
        </span>
      </div>
      {!obsAvailable ? (
        <p className="mt-3 text-sm text-muted-foreground">
          {selected.length === 0
            ? "Connect OBS to choose scenes."
            : `Connect OBS to edit ${selected.length} selected scene${selected.length === 1 ? "" : "s"}.`}
        </p>
      ) : selectable.length === 0 ? (
        <p className="mt-3 text-sm text-muted-foreground">
          OBS did not return any scenes with UUIDs.
        </p>
      ) : (
        <div className="mt-3 flex flex-col gap-2">
          <DropdownMenu>
            <DropdownMenuTrigger asChild disabled={disabled}>
              <Button variant="outline" className="w-full justify-between">
                <span className="truncate">{summary}</span>
                <span className="ml-3 text-xs text-muted-foreground">
                  {selected.length === 0 ? "Select scenes" : `${selected.length} selected`}
                </span>
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent className="w-80">
              {selectable.map((scene) => (
                <DropdownMenuCheckboxItem
                  key={scene.uuid}
                  checked={selected.includes(scene.uuid)}
                  onCheckedChange={(checked) => {
                    onChange(
                      checked
                        ? [...selected, scene.uuid]
                        : selected.filter((id) => id !== scene.uuid),
                    );
                  }}
                  onSelect={(event) => event.preventDefault()}
                >
                  {scene.name}
                </DropdownMenuCheckboxItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
          {unavailable.length > 0 && (
            <div className="flex items-center justify-between gap-3 rounded-xl border border-dashed px-3 py-2 text-sm text-muted-foreground">
              <span>
                {unavailable.length === 1
                  ? "1 selected scene is no longer in OBS."
                  : `${unavailable.length} selected scenes are no longer in OBS.`}
              </span>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={clearUnavailable}
                disabled={disabled}
              >
                Remove missing
              </Button>
            </div>
          )}
          <p className="text-sm text-muted-foreground">
            {selected.length === 0
              ? "Recording control is off until you pick at least one scene."
              : `Recording stays on for ${summary}.`}
          </p>
        </div>
      )}
    </div>
  );
}

/** A "Stop …" toggle and, while on, its minutes. */
function TimeoutSetting({
  setting,
  title,
  description,
  note,
  minutesTitle,
  minutesDescription,
  behaviour,
  saved,
  setBehaviour,
  disabled,
}: {
  setting: TimeoutKey;
  title: string;
  description: string;
  note?: string;
  minutesTitle: string;
  minutesDescription: string;
  behaviour: Behaviour;
  saved: Behaviour;
  setBehaviour: (patch: Partial<Behaviour>) => void;
  disabled: boolean;
}) {
  const minutes = behaviour[setting] ?? null;
  const inputId = `${setting}-minutes`;
  return (
    <>
      <ToggleRow
        id={`behaviour-${setting}`}
        title={title}
        description={description}
        note={note}
        checked={minutes !== null}
        onCheckedChange={(checked) =>
          setBehaviour({
            [setting]: checked ? (saved[setting] ?? TIMEOUT_FALLBACK[setting]) : null,
          })
        }
        disabled={disabled}
      />
      {minutes !== null && (
        <label
          htmlFor={inputId}
          className="flex min-h-16 items-center gap-4 border-t border-divider px-5 py-3"
        >
          <span className="flex flex-1 flex-col gap-0.5">
            <span className="text-sm font-semibold">{minutesTitle}</span>
            <span className="text-sm text-muted-foreground">{minutesDescription}</span>
          </span>
          <Input
            id={inputId}
            type="number"
            inputMode="numeric"
            min={1}
            max={1440}
            value={minutes}
            onChange={(event) => setBehaviour({ [setting]: Number(event.target.value) })}
            disabled={disabled}
            className="w-20 font-mono tabular"
          />
        </label>
      )}
    </>
  );
}
