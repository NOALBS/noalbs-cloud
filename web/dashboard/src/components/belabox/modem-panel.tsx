import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { RefreshCw } from "lucide-react";
import { type FormEvent, useId, useState } from "react";
import type { BelaboxDetails, BelaboxModemDetails } from "@/api/types";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import type { BelaboxConfig } from "@/hooks/use-belabox-config";
import {
  type ModemForm,
  modemDirty,
  modemErrors,
  modemForm,
  modemStatusLabel,
  operatorChoices,
} from "@/lib/belabox";
import { cn } from "@/lib/utils";
import { Field, Section, SignalBars } from "./field";

/** Radix Select doesn't take an empty value: "automatic" travels as this. */
const AUTOMATIC = "__automatic";

/** The BELABOX's cellular modems: APN, roaming, network type and operator. */
export function ModemPanel({
  details,
  config,
  canManage,
}: {
  details: BelaboxDetails;
  config: BelaboxConfig;
  canManage: boolean;
}) {
  if (details.modems.length === 0) {
    return (
      <p className="rounded-xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
        <Trans>This BELABOX reports no modems.</Trans>
      </p>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {details.modems.map((modem) => (
        <Modem key={modem.id} modem={modem} config={config} canManage={canManage} />
      ))}
    </div>
  );
}

function Modem({
  modem,
  config,
  canManage,
}: {
  modem: BelaboxModemDetails;
  config: BelaboxConfig;
  canManage: boolean;
}) {
  // What's being edited; `null` shows what the modem has (so the polling updates it).
  const [draft, setDraft] = useState<ModemForm | null>(null);
  const form = draft ?? modemForm(modem);
  const patch = (change: Partial<ModemForm>) => setDraft({ ...form, ...change });
  const errors = modemErrors(form, modem);
  const dirty = draft != null && modemDirty(draft, modem);
  const scanning = modem.status?.connection === "scanning";
  const settings = modem.config;
  const choices = operatorChoices(modem);
  const status = modem.status ? modemStatusLabel(modem.status) : "";

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!dirty || Object.keys(errors).length > 0) return;
    config.modem.mutate(
      {
        id: modem.id,
        autoconfig: form.autoconfig,
        apn: form.apn.trim(),
        username: form.username,
        password: form.password || null,
        roaming: form.roaming,
        network: form.network,
        network_type: form.networkType,
      },
      { onSuccess: () => setDraft(null) },
    );
  };

  return (
    <Section
      title={modem.name}
      description={
        <span className="flex items-center gap-2">
          {modem.status?.signal != null && <SignalBars signal={modem.status.signal} />}
          {modem.no_sim ? t`No SIM card` : status || modem.status?.connection || t`Not connected`}
        </span>
      }
      action={!modem.no_sim && <ScanButton modem={modem} config={config} scanning={scanning} />}
    >
      {settings && (
        <form onSubmit={submit} className="flex flex-col gap-3">
          <div className="grid gap-3 sm:grid-cols-2">
            <Field label={t`Network type`} error={errors.networkType}>
              <Select
                value={form.networkType}
                onValueChange={(networkType) => patch({ networkType })}
                disabled={!canManage}
              >
                <SelectTrigger className="w-full" aria-label={t`Network type`}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {modem.network_types.map((type) => (
                    <SelectItem key={type} value={type}>
                      {type}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
            <Field
              label={t({ context: "mobile network", message: "Operator" })}
              hint={choices.length === 0 ? t`Scan to see the operators in range.` : undefined}
            >
              <Select
                value={form.network === "" ? AUTOMATIC : form.network}
                onValueChange={(network) =>
                  patch({ network: network === AUTOMATIC ? "" : network })
                }
                disabled={!canManage}
              >
                <SelectTrigger
                  className="w-full"
                  aria-label={t({ context: "mobile network", message: "Operator" })}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={AUTOMATIC}>
                    <Trans>Automatic</Trans>
                  </SelectItem>
                  {choices.map((choice) => (
                    <SelectItem key={choice.id} value={choice.id} disabled={choice.disabled}>
                      {choice.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </Field>
          </div>

          {settings.autoconfig != null && (
            <SwitchRow
              label={t`Pick the APN automatically`}
              checked={form.autoconfig}
              onChange={(autoconfig) => patch({ autoconfig })}
              disabled={!canManage}
            />
          )}
          {!form.autoconfig && (
            <div className="grid gap-3 sm:grid-cols-3">
              <Field label="APN" error={errors.apn}>
                <Input
                  value={form.apn}
                  onChange={(event) => patch({ apn: event.target.value })}
                  disabled={!canManage}
                  autoComplete="off"
                  aria-label="APN"
                  aria-invalid={Boolean(errors.apn)}
                />
              </Field>
              <Field label={t`Username`}>
                <Input
                  value={form.username}
                  onChange={(event) => patch({ username: event.target.value })}
                  disabled={!canManage}
                  autoComplete="off"
                  aria-label={t`APN username`}
                />
              </Field>
              <Field label={t`Password`}>
                <Input
                  type="password"
                  autoComplete="new-password"
                  value={form.password}
                  onChange={(event) => patch({ password: event.target.value })}
                  placeholder={settings.has_password ? t`Unchanged` : ""}
                  disabled={!canManage}
                  aria-label={t`APN password`}
                />
              </Field>
            </div>
          )}
          <SwitchRow
            label={t`Allow roaming`}
            checked={form.roaming}
            onChange={(roaming) => patch({ roaming })}
            disabled={!canManage}
          />

          {canManage ? (
            <div className="flex items-center gap-2">
              <Button
                type="submit"
                size="sm"
                disabled={!dirty || Object.keys(errors).length > 0 || config.modem.isPending}
              >
                {config.modem.isPending ? t`Applying…` : t`Save modem`}
              </Button>
              {dirty && (
                <Button type="button" size="sm" variant="ghost" onClick={() => setDraft(null)}>
                  <Trans>Discard</Trans>
                </Button>
              )}
              <span className="text-xs text-muted-foreground">
                <Trans>The modem reconnects to apply these.</Trans>
              </span>
            </div>
          ) : (
            <p className="text-xs text-muted-foreground">
              <Trans>Only admins can change the modem.</Trans>
            </p>
          )}
        </form>
      )}
    </Section>
  );
}

function SwitchRow({
  label,
  checked,
  onChange,
  disabled,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <label
      htmlFor={id}
      className={cn(
        "flex items-center justify-between gap-3 text-sm",
        disabled ? "cursor-default" : "cursor-pointer",
      )}
    >
      {label}
      <Switch id={id} checked={checked} onCheckedChange={onChange} disabled={disabled} />
    </label>
  );
}

/** Searching for operators takes the modem offline for about a minute: ask first. */
function ScanButton({
  modem,
  config,
  scanning,
}: {
  modem: BelaboxModemDetails;
  config: BelaboxConfig;
  scanning: boolean;
}) {
  if (scanning) {
    return (
      <Button size="sm" variant="outline" disabled>
        <RefreshCw className="size-3.5 animate-spin" />
        <Trans>Scanning…</Trans>
      </Button>
    );
  }
  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button size="sm" variant="outline" disabled={config.scanModem.isPending}>
          <RefreshCw className="size-3.5" />
          <Trans>Scan operators</Trans>
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            <Trans>Scan for operators?</Trans>
          </AlertDialogTitle>
          <AlertDialogDescription>
            <Trans>
              The modem disconnects while it searches, which takes about a minute. A stream using it
              loses that connection until it’s done, so do this before you go live.
            </Trans>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>
            <Trans>Cancel</Trans>
          </AlertDialogCancel>
          <AlertDialogAction onClick={() => config.scanModem.mutate(modem.id)}>
            <Trans>Scan</Trans>
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
