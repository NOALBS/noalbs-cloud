import { useQuery } from "@tanstack/react-query";
import { SlidersHorizontal } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { belaboxDetailsQuery } from "@/api/queries";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import { useBelaboxConfig } from "@/hooks/use-belabox-config";
import { useDocumentVisible } from "@/hooks/use-document-visible";
import {
  describeResult,
  type SettingsForm,
  settingsForm,
  settingsUpdate,
  validateSettings,
} from "@/lib/belabox";
import { errorMessage } from "@/lib/errors";
import { cn } from "@/lib/utils";
import { ModemPanel } from "./modem-panel";
import { AudioPanel, StreamPanel } from "./stream-panel";
import { WifiPanel } from "./wifi-panel";

type Path = { workspace_id: string; switcher_id: string };
type Tab = "wifi" | "modems" | "audio" | "stream";

const TABS: { id: Tab; label: string }[] = [
  { id: "wifi", label: "WiFi" },
  { id: "modems", label: "Modems" },
  { id: "audio", label: "Audio & encoding" },
  { id: "stream", label: "Stream" },
];

/** How often the open dialog asks obs-link for the BELABOX's WiFi, modems and settings. */
const POLL_MS = 2000;

/**
 * Everything about the BELABOX that doesn't fit on the Live page: WiFi, modems, the audio
 * and encoder and the stream's server. The data is asked for while the dialog is open.
 * Audio and stream settings are saved together: BELABOX only keeps them when it starts, so
 * they're held here and used from its next start.
 */
export function BelaboxSettingsDialog({ path, canManage }: { path: Path; canManage: boolean }) {
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState<Tab>("wifi");
  const visible = useDocumentVisible();
  const config = useBelaboxConfig(path);

  const query = useQuery({
    ...belaboxDetailsQuery(path.workspace_id, path.switcher_id),
    enabled: open,
    refetchInterval: open && visible ? POLL_MS : false,
  });
  const details = query.data;

  // What's being edited on the audio and stream tabs; `null` shows what the BELABOX has.
  const [draft, setDraft] = useState<SettingsForm | null>(null);
  const saved = details ? settingsForm(details) : null;
  const form = draft ?? saved;
  const dirty = draft != null && saved != null && JSON.stringify(draft) !== JSON.stringify(saved);
  const errors = form && details ? validateSettings(form, details) : {};
  const invalid = Object.keys(errors).length > 0;
  const patch = (change: Partial<SettingsForm>) => form && setDraft({ ...form, ...change });

  // WiFi and hotspot requests end a while after they were sent.
  const seenResult = useRef<number | null | undefined>(undefined);
  const resultId = details?.result?.id ?? null;
  useEffect(() => {
    if (!details) return;
    // Nothing is said about a result that was already there when the dialog opened.
    if (seenResult.current === undefined) {
      seenResult.current = resultId;
      return;
    }
    if (details.result && resultId !== seenResult.current) {
      const { ok, message } = describeResult(details.result);
      (ok ? toast.success : toast.error)(message);
    }
    seenResult.current = resultId;
  }, [details, resultId]);

  const onOpenChange = (next: boolean) => {
    setOpen(next);
    if (!next) {
      setDraft(null);
      seenResult.current = undefined;
    }
  };

  const save = () => {
    if (!form || !details || invalid) return;
    config.settings.mutate(settingsUpdate(form, details), {
      onSuccess: () => {
        setDraft(null);
        toast.success("Saved. The BELABOX uses these from its next start.");
      },
    });
  };

  const editing = tab === "audio" || tab === "stream";
  const locked = Boolean(details?.streaming) || config.settings.isPending;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogTrigger asChild>
        <Button size="icon" variant="outline" className="size-8" aria-label="BELABOX settings">
          <SlidersHorizontal className="size-3.5" />
        </Button>
      </DialogTrigger>
      <DialogContent className="flex h-[min(42rem,88dvh)] flex-col gap-3 p-0 sm:max-w-2xl">
        <DialogHeader className="px-5 pt-5">
          <DialogTitle>BELABOX settings</DialogTitle>
          <DialogDescription>
            WiFi, modems, audio and the stream’s server, sent to the BELABOX through BELABOX Cloud.
          </DialogDescription>
        </DialogHeader>

        <FallbackNotice />

        <div
          role="tablist"
          aria-label="BELABOX settings"
          className="flex shrink-0 gap-1 overflow-x-auto overflow-y-hidden border-b border-divider px-5 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
        >
          {TABS.map(({ id, label }) => (
            <button
              key={id}
              type="button"
              role="tab"
              id={`belabox-tab-${id}`}
              aria-selected={tab === id}
              aria-controls="belabox-panel"
              onClick={() => setTab(id)}
              className={cn(
                "h-9 shrink-0 border-b-2 px-3 text-sm font-semibold",
                tab === id
                  ? "border-primary text-foreground"
                  : "border-transparent text-muted-foreground hover:text-foreground",
              )}
            >
              {label}
            </button>
          ))}
        </div>

        <div
          role="tabpanel"
          id="belabox-panel"
          aria-labelledby={`belabox-tab-${tab}`}
          className="min-h-0 flex-1 overflow-y-auto px-5 pb-1"
        >
          {query.isPending && open && (
            <div className="flex flex-col gap-3">
              <Skeleton className="h-24 rounded-xl" />
              <Skeleton className="h-32 rounded-xl" />
            </div>
          )}
          {query.isError && !details && (
            <p className="rounded-xl border border-dashed px-4 py-6 text-sm text-muted-foreground">
              {errorMessage(query.error)}
            </p>
          )}
          {details && form && (
            <div className="flex flex-col gap-3">
              {editing && <Notice details={details} />}
              {tab === "wifi" && (
                <WifiPanel details={details} config={config} canManage={canManage} />
              )}
              {tab === "modems" && (
                <ModemPanel details={details} config={config} canManage={canManage} />
              )}
              {tab === "audio" && (
                <AudioPanel
                  details={details}
                  form={form}
                  errors={errors}
                  patch={patch}
                  disabled={locked}
                />
              )}
              {tab === "stream" && (
                <StreamPanel
                  details={details}
                  form={form}
                  errors={errors}
                  patch={patch}
                  disabled={locked}
                />
              )}
            </div>
          )}
        </div>

        {editing && dirty && (
          <div className="flex items-center gap-3 border-t border-divider px-5 py-3">
            <span
              className={cn(
                "flex-1 text-sm",
                invalid ? "text-destructive-foreground" : "text-muted-foreground",
              )}
            >
              {invalid ? "Fix the highlighted fields to save." : "Unsaved changes"}
            </span>
            <Button variant="ghost" size="sm" onClick={() => setDraft(null)}>
              Discard
            </Button>
            <Button size="sm" onClick={save} disabled={invalid || locked}>
              {config.settings.isPending ? "Saving…" : "Save"}
            </Button>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

/** Why the audio and stream settings may not apply yet. */
function Notice({ details }: { details: { streaming: boolean; settings_pending: boolean } }) {
  if (details.streaming) {
    return (
      <p className="rounded-xl bg-low/12 px-3 py-2 text-sm text-low">
        The BELABOX is streaming. Stop it to change these settings.
      </p>
    );
  }
  return (
    <p className="rounded-xl bg-surface-2 px-3 py-2 text-sm text-muted-foreground">
      {details.settings_pending
        ? "Saved here, not on the BELABOX yet: it keeps them when it starts from the dashboard, chat or the OBS stream. Starting it from its own screen discards them."
        : "The BELABOX only saves these when it starts a stream, so changes are used from its next start from here."}
    </p>
  );
}

/**
 * These orders change the BELABOX itself, and we can't undo them (a wrong WiFi or modem
 * setting can leave it offline), so say where to go if something breaks.
 */
function FallbackNotice() {
  return (
    <p className="mx-5 shrink-0 rounded-xl bg-low/12 px-3 py-2 text-xs text-low">
      Changes here go straight to the BELABOX and can’t be undone from NOALBS. If something breaks,
      for example it can’t get online anymore, use the <strong>official BELABOX remote</strong>{" "}
      (with your secret URL) or the BELABOX’s local UI to fix it.
    </p>
  );
}
