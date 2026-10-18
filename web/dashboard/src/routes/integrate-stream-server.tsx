import { createFileRoute, Link } from "@tanstack/react-router";
import { useState } from "react";
import type { StreamServerKind } from "@/api/types";
import { CopyField } from "@/components/copy-field";
import {
  type DetailsForm,
  detailsForm,
  ServerDetailsFields,
} from "@/components/server-details-fields";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Wordmark } from "@/components/wordmark";
import {
  ADD_SERVER_PATH,
  buildAddServerFragment,
  detailFieldsOf,
  displayHost,
  LINK_DETAIL_FIELDS,
} from "@/lib/add-server";
import { kindInfo, STREAM_SERVER_KINDS } from "@/lib/stream-server-kinds";

export const Route = createFileRoute("/integrate-stream-server")({
  component: IntegratePage,
});

const EXAMPLE_URLS: Partial<Record<StreamServerKind, string>> = {
  belabox: "https://stats.srt.belabox.net/your-key",
  irlhosting_websocket: "wss://your-server.example/ws-stats",
};

/** For stream server providers: how to link to "Add to NOALBS Cloud", with a builder. */
function IntegratePage() {
  const [kind, setKind] = useState<StreamServerKind>("belabox");
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [details, setDetails] = useState<DetailsForm>(detailsForm());

  const info = kindInfo(kind);
  const link = `${window.location.origin}${ADD_SERVER_PATH}#${buildAddServerFragment({
    kind,
    url: url || (EXAMPLE_URLS[kind] ?? info.placeholder),
    name,
    details: Object.fromEntries(LINK_DETAIL_FIELDS.map((field) => [field, details[field]])),
  })}`;
  const snippet = `<a href="${link.replaceAll("&", "&amp;")}">Add to NOALBS Cloud</a>`;

  return (
    <div className="flex min-h-dvh flex-col">
      <header className="flex h-16 items-center justify-between border-b border-divider px-4 md:px-8">
        <Link to="/" aria-label="Home">
          <Wordmark className="text-base" />
        </Link>
      </header>
      <main className="mx-auto flex w-full max-w-3xl flex-1 flex-col gap-8 px-4 py-10">
        <div className="flex flex-col gap-2.5">
          <h1 className="text-3xl font-bold tracking-tight">Add to NOALBS Cloud</h1>
          <p className="text-muted-foreground">
            Running a stream server? Put a link on your site that opens NOALBS Cloud with your stats
            URL filled in. The streamer signs in, picks a workspace and a switcher, and confirms.
          </p>
        </div>

        <section className="flex flex-col gap-3">
          <h2 className="text-lg font-semibold">The link</h2>
          <CopyField
            value={`${window.location.origin}${ADD_SERVER_PATH}#kind=…&url=…`}
            label="Link format"
          />
          <ul className="flex list-disc flex-col gap-1.5 pl-5 text-sm text-muted-foreground">
            <li>
              Everything is in the part after the <code className="font-mono">#</code>, which
              browsers never send to a server, so stats URLs and keys stay out of our logs. Encode
              the values (<code className="font-mono">encodeURIComponent</code>).
            </li>
            <li>
              <code className="font-mono">kind</code> and <code className="font-mono">url</code> are
              required, <code className="font-mono">name</code> is optional (the streamer can change
              it). Settings a kind doesn’t use are ignored.
            </li>
            <li>
              Kinds:{" "}
              {STREAM_SERVER_KINDS.map((option, index) => (
                <span key={option.kind}>
                  {index > 0 && ", "}
                  <code className="font-mono">{option.kind}</code>
                </span>
              ))}
              .
            </li>
            <li>
              Other settings by kind: <code className="font-mono">publisher</code>,{" "}
              <code className="font-mono">application</code>, <code className="font-mono">key</code>
              , <code className="font-mono">listener_id</code>,{" "}
              <code className="font-mono">username</code>,{" "}
              <code className="font-mono">password</code> and{" "}
              <code className="font-mono">api_key</code> (a token for IRLHosting WebSocket). The
              builder below shows which ones each kind takes.
            </li>
          </ul>
        </section>

        <section className="flex flex-col gap-5 rounded-3xl border bg-card p-6">
          <h2 className="text-lg font-semibold">Link builder</h2>

          <div className="flex flex-col gap-2">
            <Label htmlFor="b-kind">Kind</Label>
            <Select
              value={kind}
              onValueChange={(next) => {
                setKind(next as StreamServerKind);
                setDetails(detailsForm());
              }}
            >
              <SelectTrigger id="b-kind" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {STREAM_SERVER_KINDS.map((option) => (
                  <SelectItem key={option.kind} value={option.kind}>
                    {option.label} ({option.blurb})
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="b-url">Stats URL</Label>
            <Input
              id="b-url"
              value={url}
              onChange={(event) => setUrl(event.target.value)}
              placeholder={EXAMPLE_URLS[kind] ?? info.placeholder}
              className="font-mono"
              spellCheck={false}
            />
            <p className="text-xs text-muted-foreground">{info.help}</p>
          </div>

          <div className="flex flex-col gap-2">
            <Label htmlFor="b-name">
              Name <span className="font-normal text-muted-foreground">(optional)</span>
            </Label>
            <Input
              id="b-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={info.label}
              maxLength={60}
            />
          </div>

          <ServerDetailsFields kind={kind} form={details} onChange={setDetails} idPrefix="b" />
          {detailFieldsOf(kind).length === 0 && (
            <p className="text-xs text-muted-foreground">This kind has no other settings.</p>
          )}

          <div className="flex flex-col gap-2">
            <Label>Link</Label>
            <CopyField value={link} label="Link" />
            {url && displayHost(url) === null && (
              <p className="text-xs text-destructive">
                The stats URL must start with{" "}
                {info.label.includes("WebSocket") ? "ws:// or wss://" : "http:// or https://"}.
              </p>
            )}
          </div>

          <div className="flex flex-col gap-2">
            <Label>HTML</Label>
            <CopyField value={snippet} label="HTML" />
          </div>

          <a
            href={link}
            target="_blank"
            rel="noreferrer"
            className="text-sm font-semibold text-primary hover:underline"
          >
            Try it
          </a>
        </section>
      </main>
    </div>
  );
}
