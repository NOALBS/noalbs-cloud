import type { BelaboxDetails } from "@/api/types";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
  AUDIO_DELAY_MAX,
  AUDIO_DELAY_MIN,
  type SettingsErrors,
  type SettingsForm,
  SRT_LATENCY_MAX,
  SRT_LATENCY_MIN,
  selectedPipeline,
} from "@/lib/belabox";
import { cn } from "@/lib/utils";
import { Field, Section } from "./field";

interface FormProps {
  details: BelaboxDetails;
  form: SettingsForm;
  errors: SettingsErrors;
  patch: (change: Partial<SettingsForm>) => void;
  /** Streaming, or saving. */
  disabled: boolean;
}

/** Radix Select doesn't take an empty value: "no relay account" travels as this. */
const NO_ACCOUNT = "__none";

/** Encoder, audio source and codec, audio delay and the bitrate overlay. */
export function AudioPanel({ details, form, errors, patch, disabled }: FormProps) {
  const pipeline = selectedPipeline(details, form);
  // The saved source may be an input that isn't plugged in right now.
  const sources = details.audio_sources.includes(form.audioSource)
    ? details.audio_sources
    : form.audioSource
      ? [form.audioSource, ...details.audio_sources]
      : details.audio_sources;

  return (
    <div className="flex flex-col gap-3">
      <Section title="Encoder" description="What the BELABOX encodes the picture with.">
        <Field label="Pipeline" error={errors.pipeline}>
          <Select
            value={form.pipeline}
            onValueChange={(value) => patch({ pipeline: value })}
            disabled={disabled}
          >
            <SelectTrigger className="w-full" aria-label="Pipeline">
              <SelectValue placeholder="Pick a pipeline" />
            </SelectTrigger>
            <SelectContent>
              {details.pipelines.map((option) => (
                <SelectItem key={option.id} value={option.id}>
                  {option.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
        <label
          htmlFor="belabox-overlay"
          className="flex items-center justify-between gap-3 text-sm"
        >
          <span className="flex flex-col">
            Bitrate overlay
            <span className="text-xs text-muted-foreground">
              Shows the bitrate on the video. Leave it off for a stream.
            </span>
          </span>
          <Switch
            id="belabox-overlay"
            checked={form.bitrateOverlay}
            onCheckedChange={(bitrateOverlay) => patch({ bitrateOverlay })}
            disabled={disabled}
            aria-label="Bitrate overlay"
          />
        </label>
      </Section>

      <Section
        title="Audio"
        description={
          pipeline && !pipeline.audio_source && !pipeline.audio_codec
            ? "This pipeline has no audio settings."
            : undefined
        }
      >
        <div className="grid gap-3 sm:grid-cols-2">
          {pipeline?.audio_source && (
            <Field label="Audio source" error={errors.audioSource}>
              <Select
                value={form.audioSource}
                onValueChange={(audioSource) => patch({ audioSource })}
                disabled={disabled}
              >
                <SelectTrigger className="w-full" aria-label="Audio source">
                  <SelectValue placeholder="Pick a source" />
                </SelectTrigger>
                <SelectContent>
                  {sources.map((source) => (
                    <SelectItem key={source} value={source}>
                      {source}
                      {!details.audio_sources.includes(source) && " (not found now)"}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
          )}
          {pipeline?.audio_codec && (
            <Field label="Audio codec" error={errors.audioCodec}>
              <Select
                value={form.audioCodec}
                onValueChange={(audioCodec) => patch({ audioCodec })}
                disabled={disabled}
              >
                <SelectTrigger className="w-full" aria-label="Audio codec">
                  <SelectValue placeholder="Pick a codec" />
                </SelectTrigger>
                <SelectContent>
                  {details.audio_codecs.map((codec) => (
                    <SelectItem key={codec.id} value={codec.id}>
                      {codec.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
          )}
          <Field
            label="Audio delay (ms)"
            htmlFor="belabox-audio-delay"
            error={errors.audioDelay}
            hint={`Shifts the audio against the video, ${AUDIO_DELAY_MIN} to ${AUDIO_DELAY_MAX}.`}
          >
            <Input
              id="belabox-audio-delay"
              inputMode="numeric"
              value={form.audioDelay}
              onChange={(event) => patch({ audioDelay: event.target.value.replace(/[^\d-]/g, "") })}
              disabled={disabled}
              className="font-mono tabular"
              aria-invalid={Boolean(errors.audioDelay)}
            />
          </Field>
        </div>
      </Section>
    </div>
  );
}

/** SRT latency, the server to send to and the stream ID. */
export function StreamPanel({ details, form, errors, patch, disabled }: FormProps) {
  const usesAccount = form.server === "relay" && form.relayAccount !== "";

  return (
    <div className="flex flex-col gap-3">
      <Section title="Latency">
        <Field
          label="SRT latency (ms)"
          htmlFor="belabox-srt-latency"
          error={errors.srtLatency}
          hint={`More latency survives worse connections, ${SRT_LATENCY_MIN} to ${SRT_LATENCY_MAX}.`}
        >
          <Input
            id="belabox-srt-latency"
            inputMode="numeric"
            value={form.srtLatency}
            onChange={(event) => patch({ srtLatency: event.target.value.replace(/\D/g, "") })}
            disabled={disabled}
            className="max-w-40 font-mono tabular"
            aria-invalid={Boolean(errors.srtLatency)}
          />
        </Field>
      </Section>

      <Section title="Server" description="Where the BELABOX sends the stream.">
        <div
          role="radiogroup"
          aria-label="Server"
          className="flex gap-1.5 rounded-lg bg-surface-2 p-1"
        >
          {(
            [
              ["relay", "BELABOX relay"],
              ["own", "Own server"],
            ] as const
          ).map(([server, label]) => (
            <button
              key={server}
              type="button"
              aria-pressed={form.server === server}
              disabled={disabled}
              onClick={() => patch({ server })}
              className={cn(
                "h-8 flex-1 rounded-md px-3 text-sm font-semibold disabled:opacity-50",
                form.server === server
                  ? "bg-background shadow-xs"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              {label}
            </button>
          ))}
        </div>

        {form.server === "relay" ? (
          <div className="grid gap-3 sm:grid-cols-2">
            <Field label="Relay server" error={errors.relayServer}>
              <Select
                value={form.relayServer}
                onValueChange={(relayServer) => patch({ relayServer })}
                disabled={disabled}
              >
                <SelectTrigger className="w-full" aria-label="Relay server">
                  <SelectValue placeholder="Pick a server" />
                </SelectTrigger>
                <SelectContent>
                  {details.relay_servers.map((server) => (
                    <SelectItem key={server.id} value={server.id}>
                      {server.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
            <Field label="Relay account" error={errors.relayAccount}>
              <Select
                value={form.relayAccount === "" ? NO_ACCOUNT : form.relayAccount}
                onValueChange={(value) =>
                  patch({ relayAccount: value === NO_ACCOUNT ? "" : value })
                }
                disabled={disabled}
              >
                <SelectTrigger className="w-full" aria-label="Relay account">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={NO_ACCOUNT}>Use a stream ID instead</SelectItem>
                  {details.relay_accounts.map((account) => (
                    <SelectItem key={account.id} value={account.id} disabled={account.disabled}>
                      {account.name}
                      {account.disabled && " (disabled)"}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
          </div>
        ) : (
          <div className="grid gap-3 sm:grid-cols-[1fr_8rem]">
            <Field label="SRTLA address" error={errors.address}>
              <Input
                value={form.address}
                onChange={(event) => patch({ address: event.target.value })}
                disabled={disabled}
                autoComplete="off"
                spellCheck={false}
                aria-label="SRTLA address"
                aria-invalid={Boolean(errors.address)}
              />
            </Field>
            <Field label="Port" error={errors.port}>
              <Input
                inputMode="numeric"
                value={form.port}
                onChange={(event) => patch({ port: event.target.value.replace(/\D/g, "") })}
                disabled={disabled}
                className="font-mono tabular"
                aria-label="SRTLA port"
                aria-invalid={Boolean(errors.port)}
              />
            </Field>
          </div>
        )}

        {!usesAccount && (
          <Field
            label="Stream ID"
            error={errors.streamId}
            hint={
              details.settings.has_stream_id
                ? "Leave empty to keep the one the BELABOX has."
                : "The key your server gives you for this stream."
            }
          >
            <Input
              type="password"
              autoComplete="new-password"
              value={form.streamId}
              onChange={(event) => patch({ streamId: event.target.value })}
              placeholder={details.settings.has_stream_id ? "Unchanged" : ""}
              disabled={disabled}
              aria-label="Stream ID"
              aria-invalid={Boolean(errors.streamId)}
            />
          </Field>
        )}
      </Section>
    </div>
  );
}
