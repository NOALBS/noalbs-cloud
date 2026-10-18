import type { StreamServerKind } from "@/api/types";

/** A kind-specific setting kept in the server's `details`. */
export type DetailField = "publisher" | "application" | "key" | "listener_id";

export interface KindInfo {
  kind: StreamServerKind;
  label: string;
  /** What it is, in a few words. */
  blurb: string;
  placeholder: string;
  help: string;
  /** Settings it needs; `optional` ones may be left empty. Mirrors the API. */
  fields: { field: DetailField; optional?: boolean }[];
  /** What this kind calls a field, instead of `FIELD_LABELS`. */
  fieldLabels?: Partial<Record<DetailField, { label: string; placeholder: string }>>;
  /** Takes an optional basic-auth username and password. */
  basicAuth?: boolean;
  /** Takes an optional API key. */
  apiKey?: boolean;
  /** What this kind calls its API key (default "API key"). */
  apiKeyLabel?: string;
  /** Extra guidance for its settings. */
  fieldsHelp?: string;
}

export const STREAM_SERVER_KINDS: KindInfo[] = [
  {
    kind: "belabox",
    label: "BELABOX",
    blurb: "BELABOX Cloud",
    placeholder: "https://stats.srt.belabox.net/your-key",
    help: "The stats URL of your ingest, from BELABOX Cloud.",
    fields: [],
  },
  {
    kind: "mediamtx",
    label: "MediaMTX",
    blurb: "SRT / RTMP",
    placeholder: "http://your-server:9997/v3/paths/get/live",
    help: "The MediaMTX API address of your stream's path. Enable the API in mediamtx.yml.",
    fields: [],
    basicAuth: true,
  },
  {
    kind: "sls",
    label: "SLS",
    blurb: "SRT Live Server",
    placeholder: "http://your-server:8181/stats",
    help: "The /stats page of SRT Live Server.",
    fields: [{ field: "publisher" }],
    apiKey: true,
  },
  {
    kind: "nginx",
    label: "NGINX",
    blurb: "nginx-rtmp",
    placeholder: "http://your-server/stat",
    help: "The rtmp_stat page (XML) of NGINX with the RTMP module.",
    fields: [{ field: "application" }, { field: "key" }],
  },
  {
    kind: "nms",
    label: "NMS",
    blurb: "Node Media Server",
    placeholder: "http://your-server:8000/api/streams",
    help: "The streams API of Node Media Server; the application and key are added to it.",
    fields: [{ field: "application" }, { field: "key" }],
    basicAuth: true,
  },
  {
    kind: "nimble",
    label: "Nimble",
    blurb: "Nimble Streamer",
    placeholder: "http://your-server:8082",
    help: "The address of Nimble's API (without /manage/…).",
    fields: [{ field: "listener_id" }, { field: "application" }, { field: "key" }],
    fieldsHelp: "Nimble needs the SRT stream republished as RTMP; the bitrate comes from that.",
  },
  {
    kind: "xiu",
    label: "Xiu",
    blurb: "Xiu media server",
    placeholder: "http://your-server:8000/api/query_stream",
    help: "Xiu's query_stream API.",
    fields: [{ field: "application" }, { field: "key" }],
  },
  {
    kind: "openirl",
    label: "OpenIRL",
    blurb: "OpenIRL SRT",
    placeholder: "http://your-server:8080/stats/play_…",
    help: "The stats URL of your OpenIRL stream.",
    fields: [],
  },
  {
    kind: "openrtmp",
    label: "OpenRTMP",
    blurb: "OpenRTMP",
    placeholder: "https://your-server/stats?key=…",
    help: "The stats URL including your stats key.",
    fields: [],
  },
  {
    kind: "rist",
    label: "RIST",
    blurb: "RIST receiver",
    placeholder: "http://your-server:8080/stats",
    help: "The stats page of your RIST receiver.",
    fields: [],
  },
  {
    kind: "irlhosting",
    label: "IRLHosting",
    blurb: "SRT or RTMP",
    placeholder: "http://your-server:8181/stats",
    help: "The stats URL from IRLHosting.",
    fields: [
      { field: "publisher", optional: true },
      { field: "application", optional: true },
      { field: "key", optional: true },
    ],
    fieldsHelp: "For SRT fill in the publisher; for RTMP the application and stream key.",
  },
  {
    kind: "irlhosting_websocket",
    label: "IRLHosting WebSocket",
    blurb: "Live stats push",
    placeholder: "wss://your-server/ws-stats",
    help: "The stats WebSocket from IRLHosting (ws:// or wss://). It sends the stats as they change.",
    fields: [{ field: "publisher", optional: true }],
    fieldLabels: { publisher: { label: "Feed", placeholder: "feed1" } },
    apiKey: true,
    apiKeyLabel: "Token",
    fieldsHelp: "Leave the feed empty to use the first active stream.",
  },
];

export const FIELD_LABELS: Record<DetailField, { label: string; placeholder: string }> = {
  publisher: { label: "Publisher (stream ID)", placeholder: "publish/live/feed1" },
  application: { label: "Application", placeholder: "live" },
  key: { label: "Stream key", placeholder: "feed1" },
  listener_id: { label: "SRT listener ID", placeholder: "0.0.0.0:2020" },
};

export function kindInfo(kind: StreamServerKind): KindInfo {
  return (
    STREAM_SERVER_KINDS.find((info) => info.kind === kind) ?? (STREAM_SERVER_KINDS[0] as KindInfo)
  );
}
