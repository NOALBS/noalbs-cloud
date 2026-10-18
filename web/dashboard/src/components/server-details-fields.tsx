import type { ServerDetails, ServerDetailsInput, StreamServerKind } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { type DetailField, FIELD_LABELS, kindInfo } from "@/lib/stream-server-kinds";
import { SecretUrlInput } from "./secret-url-input";

/** The kind-specific settings being edited. Secrets start empty (write-only). */
export interface DetailsForm {
  publisher: string;
  application: string;
  key: string;
  listener_id: string;
  username: string;
  password: string;
  clear_password: boolean;
  api_key: string;
  clear_api_key: boolean;
}

export function detailsForm(details?: ServerDetails | null): DetailsForm {
  return {
    publisher: details?.publisher ?? "",
    application: details?.application ?? "",
    key: details?.key ?? "",
    listener_id: details?.listener_id ?? "",
    username: details?.username ?? "",
    password: "",
    clear_password: false,
    api_key: "",
    clear_api_key: false,
  };
}

/** What to send: blank fields as null, secrets only when typed or cleared. */
export function detailsInput(form: DetailsForm): ServerDetailsInput {
  const text = (value: string) => value.trim() || null;
  return {
    publisher: text(form.publisher),
    application: text(form.application),
    key: text(form.key),
    listener_id: text(form.listener_id),
    username: text(form.username),
    password: form.password || null,
    clear_password: form.clear_password,
    api_key: form.api_key || null,
    clear_api_key: form.clear_api_key,
  };
}

export function sameDetails(a: DetailsForm, b: DetailsForm): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/**
 * The settings a kind of stream server needs beyond its stats URL, e.g. the
 * RTMP application and stream key, or MediaMTX's username and password.
 */
export function ServerDetailsFields({
  kind,
  form,
  onChange,
  stored,
  idPrefix,
  disabled = false,
}: {
  kind: StreamServerKind;
  form: DetailsForm;
  onChange: (form: DetailsForm) => void;
  /** What's saved, to show "a password is stored". */
  stored?: ServerDetails | null;
  idPrefix: string;
  disabled?: boolean;
}) {
  const info = kindInfo(kind);
  if (info.fields.length === 0 && !info.basicAuth && !info.apiKey) return null;

  const set = (patch: Partial<DetailsForm>) => onChange({ ...form, ...patch });
  const id = (field: string) => `${idPrefix}-${field}`;

  return (
    <div className="flex flex-col gap-4">
      {info.fields.length > 0 && (
        <div className="grid gap-4 sm:grid-cols-2">
          {info.fields.map(({ field, optional }) => (
            <DetailInput
              key={field}
              id={id(field)}
              field={field}
              labels={info.fieldLabels?.[field] ?? FIELD_LABELS[field]}
              optional={optional}
              value={form[field]}
              onChange={(value) => set({ [field]: value })}
              disabled={disabled}
            />
          ))}
        </div>
      )}
      {info.fieldsHelp && <p className="-mt-2 text-xs text-muted-foreground">{info.fieldsHelp}</p>}

      {info.basicAuth && (
        <div className="grid gap-4 sm:grid-cols-2">
          <div className="flex flex-col gap-2">
            <Label htmlFor={id("username")}>
              Username <Optional />
            </Label>
            <Input
              id={id("username")}
              value={form.username}
              onChange={(event) => set({ username: event.target.value })}
              autoComplete="off"
              spellCheck={false}
              disabled={disabled}
            />
          </div>
          <SecretField
            id={id("password")}
            label="Password"
            value={form.password}
            cleared={form.clear_password}
            isStored={Boolean(stored?.has_password)}
            onChange={(password) => set({ password, clear_password: false })}
            onClear={(clear_password) => set({ clear_password, password: "" })}
            disabled={disabled}
          />
        </div>
      )}

      {info.apiKey && (
        <SecretField
          id={id("api-key")}
          label={info.apiKeyLabel ?? "API key"}
          value={form.api_key}
          cleared={form.clear_api_key}
          isStored={Boolean(stored?.has_api_key)}
          onChange={(api_key) => set({ api_key, clear_api_key: false })}
          onClear={(clear_api_key) => set({ clear_api_key, api_key: "" })}
          disabled={disabled}
        />
      )}
    </div>
  );
}

function Optional() {
  return <span className="font-normal text-muted-foreground">(optional)</span>;
}

function DetailInput({
  id,
  field,
  labels: { label, placeholder },
  optional,
  value,
  onChange,
  disabled,
}: {
  id: string;
  field: DetailField;
  labels: { label: string; placeholder: string };
  optional?: boolean;
  value: string;
  onChange: (value: string) => void;
  disabled: boolean;
}) {
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor={id}>
        {label} {optional && <Optional />}
      </Label>
      {field === "key" ? (
        // A stream key lets anyone publish to your ingest: hidden like the stats URL.
        <SecretUrlInput
          id={id}
          value={value}
          onChange={onChange}
          placeholder={placeholder}
          label="stream key"
          mask={() => "••••••••"}
          readOnly={disabled}
        />
      ) : (
        <Input
          id={id}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={placeholder}
          className="font-mono"
          autoComplete="off"
          spellCheck={false}
          disabled={disabled}
        />
      )}
    </div>
  );
}

/** A write-only secret: type a new one, keep the stored one, or remove it. */
function SecretField({
  id,
  label,
  value,
  cleared,
  isStored,
  onChange,
  onClear,
  disabled,
}: {
  id: string;
  label: string;
  value: string;
  cleared: boolean;
  isStored: boolean;
  onChange: (value: string) => void;
  onClear: (clear: boolean) => void;
  disabled: boolean;
}) {
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor={id}>
        {label} <Optional />
      </Label>
      <div className="flex gap-2">
        <Input
          id={id}
          type="password"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={
            cleared
              ? "Will be removed"
              : isStored
                ? disabled
                  ? "Saved"
                  : "Saved. Type to replace"
                : undefined
          }
          autoComplete="new-password"
          className="min-w-0 flex-1"
          disabled={disabled}
        />
        {isStored && (
          <Button
            type="button"
            variant="outline"
            onClick={() => onClear(!cleared)}
            disabled={disabled}
          >
            {cleared ? "Keep" : "Remove"}
          </Button>
        )}
      </div>
    </div>
  );
}
