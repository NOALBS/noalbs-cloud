# Deploying NOALBS Cloud

Everything runs from the images CI publishes on `main`
(`ghcr.io/noalbs/noalbs-cloud/<service>:latest`, plus `:sha-<commit>`):

| Service | What it does |
|---|---|
| `dashboard` | Caddy serving the web app and the OBS browser source (`/source/`). It also forwards `/api` to the API, `/link` and `/moblin/*` to obs-link and `/chat/webhooks` to the chat service, so one domain is enough. |
| `api` | The HTTP API (port 12000): sign-in, workspaces, switcher settings, live data. |
| `obs-link` | The WebSocket the OBS browser sources connect to (port 11000). It runs the switching. |
| `chat` | Reads Twitch, Kick and YouTube chat and runs the chat commands (port 13000, for Kick's webhook). Run one or several. |
| `db-migrator` | Runs the database migrations, then exits. |
| Postgres 18 | Everything that's stored. |
| NATS 2.11+ with JetStream | Live status, stats and commands between the API and obs-link. |

## Quick start (one machine, Docker Compose)

```sh
cd deploy
cp .env.example .env
# Fill in .env: your URL, and a secret for each of these:
openssl rand -hex 32   # POSTGRES_PASSWORD, COOKIE_SECRET, DATA_ENCRYPTION_KEY (one each)
docker compose up -d
```

`db-migrator` runs first; the API and obs-link start once it has finished.

**Keep `DATA_ENCRYPTION_KEY` safe and unchanged.** It encrypts stored OBS and stream
server passwords and BELABOX remote keys; with another key they can't be read (people would have to enter
them again). The API and obs-link must use the same one. Release builds refuse to
start with the example key from the docs.

### HTTPS

Pick one:

- **Let the dashboard container do it.** Set `SITE_ADDRESS=noalbs.example.com`, point the
  domain's DNS at the machine and open ports 80 and 443. Caddy gets and renews the
  certificate (stored in the `caddy` volume).
- **Your own proxy terminates TLS** (Traefik, nginx, a load balancer, Cloudflare…). Keep
  `SITE_ADDRESS=:80` and send all traffic for the domain to the dashboard container's port
  80 (`HTTP_PORT` changes the published port).
  The API rate-limits per visitor, so set `CLIENT_IP_HEADER` to the header your proxy puts
  the visitor's IP in (Cloudflare: `cf-connecting-ip`); otherwise everyone shares the
  proxy's address. Only do that when nobody can reach port 80 around the proxy, or they
  could pick any IP.

Either way `NOALBS_PUBLIC_URL` is `https://…`. The OBS browser source connects to `wss://…/link` on the same domain.
Sign-in cookies are `Secure`, so production must be HTTPS.

### Sign-in (Twitch, Kick and YouTube)

Create an OAuth app in the [Twitch developer console](https://dev.twitch.tv/console/apps),
Kick's developer settings and/or Google Cloud (below), and put the client ID and secret in `.env`. A provider
without both values isn't offered. Register **all three** redirect URLs per provider:

```
https://noalbs.example.com/api/auth/twitch/callback
https://noalbs.example.com/api/auth/link/twitch/callback      # connecting it to an existing account
https://noalbs.example.com/api/chat/connect/twitch/callback    # chat bot: channels and bot accounts
https://noalbs.example.com/api/auth/kick/callback
https://noalbs.example.com/api/auth/link/kick/callback
https://noalbs.example.com/api/chat/connect/kick/callback
https://noalbs.example.com/api/auth/youtube/callback
https://noalbs.example.com/api/auth/link/youtube/callback
https://noalbs.example.com/api/chat/connect/youtube/callback
```

The same apps are used by the chat bot, so its channel grants belong to them.

**YouTube** is a Google Cloud project: enable the **YouTube Data API v3**, set up the OAuth
consent screen with the scopes `youtube.readonly` (sign-in, and streamers' channels: finding
their live broadcast and reading its chat) and `youtube.force-ssl` (bot accounts: posting),
then create an OAuth client of type *Web application* with the redirect URLs above, and an
API key restricted to the YouTube Data API (optional: it looks channels up by handle for
ignored chatters). Both scopes are *sensitive*: until Google has verified the app, only test
users you add can connect, and their grants expire after 7 days. The account is a YouTube
channel (a Google account with several picks one on Google's screen). YouTube's API has a
**daily quota** (10,000 units by default, per project): each bot message costs 50, each
look for a live broadcast 1, so ask Google for more before many streamers use it; Admin →
Status shows when it ran out.

### Staff and the admin section

Staff see an **Admin** item in their account menu:
- an overview (users, workspaces, paying workspaces, monthly revenue)
- live status: switchers connected and live now, the chat service instances, Twitch's conduit
  shards and EventSub cost, Kick subscriptions, and chat grants that need reconnecting
- users and workspaces, with the actions to comp a plan, open any workspace, and grant or
  remove staff
- the plans and add-ons for sale
- the shared chat bots (see [Chat bot](#chat-bot))

The first staff account has to be made in the database. Sign in once, then run:

```sh
docker compose exec postgres psql -U noalbs noalbs -c \
  "UPDATE users SET staff = true WHERE id = (SELECT user_id FROM oauth2_connections WHERE provider_username = 'your_name')"
```

From then on, staff can grant it to others under Admin → Users.

### Billing (Stripe)

Without Stripe settings, billing is off: the plan page says plan changes open soon, and
staff can still move workspaces between plans by hand. To sell Pro:

1. Put a Stripe secret key in `STRIPE_SECRET_KEY`: `sk_live_…`, or a restricted key
   allowed to write Customers, Checkout Sessions, customer portal sessions, Subscriptions,
   Products and Prices.
   - Products and prices are set up in NOALBS itself, under **Admin → Products** (staff
     only; open it from the account menu). Setting a price there creates it in Stripe.
     Changing it creates a new price; people already paying keep their old one.
   - A price you made in the Stripe Dashboard can be linked by its ID instead.
   - Price IDs differ between test and live mode, so a test-mode key needs a database
     of its own.
2. Add a **webhook endpoint** at `https://noalbs.example.com/api/billing/webhook` with
   these events, and put its signing secret (`whsec_…`) in `STRIPE_WEBHOOK_SECRET`:
   `checkout.session.completed`, `customer.subscription.created`,
   `customer.subscription.updated`, `customer.subscription.deleted`,
   `customer.subscription.paused`, `customer.subscription.resumed`, `invoice.paid`,
   `invoice.payment_failed`.
3. Set up the **customer portal** (Settings → Billing → Customer portal): allow updating
   payment methods and viewing invoices, and cancelling **at the end of the period**. Plan,
   add-on and billing-period changes happen on NOALBS's own Plan page (the portal can't
   change subscriptions with add-ons), so plan switching in the portal can stay off.
4. Choose the **grace period** under Settings → Billing → Subscriptions and emails →
   Manage failed payments: while Stripe retries a failed payment the subscription is
   `past_due` and the workspace keeps Pro (the owner sees a warning). When retries run out,
   cancel the subscription (or mark it unpaid); the workspace then drops to Free. Retrying
   for about two weeks is a good default.
5. Turn on **promotion codes** in Checkout by creating coupons and promotion codes in the
   Dashboard; the Checkout page shows a code field.

**Add-ons** (such as Ingest) are sold per unit on top of any plan, including Free. Give them
prices under Admin → Products and set them to **Available**. The other states are **Hidden**
(not listed, except to workspaces that already have it), **Coming soon** (listed, not for
sale yet) and **Currently unavailable** (listed, sold before but paused: nobody can buy
more, those who have it keep it). An Available add-on without a price shows as coming soon.
Plans and add-ons nobody uses (no workspaces on the plan, no live subscription paying for
or holding it) can be removed there too; their Stripe product and prices are archived. Adding (more of) an
add-on or a plan is charged right away for the rest of the period; lowering or removing it
applies at the end of the period, without a refund.

A workspace that drops to Free keeps everything, but switchers beyond the Free limit
(newest first) are **paused**: they stay connected and don't switch until the workspace
upgrades again or deletes other switchers. If someone pays twice at once (two Checkout tabs),
the API logs "Workspace already has a live subscription": refund and cancel one of them in
Stripe.

### Chat bot

Streamers add their Twitch, Kick or YouTube channels on a switcher's **Chat & commands** page by signing
in as the channel's owner; that lets NOALBS's bot into the channel. Replies come from a
**shared bot account** per platform: create a Twitch and a Kick account and a YouTube channel
for it (e.g. `NOALBSBot`), then under **Admin → Chat bots** press Connect and sign in as that account
(not your own). Plans with **Own chat bot** turned on (Pro by default) let a workspace
connect its own bot account the same way, from the Chat page.

The **chat service** reads the linked channels' chat and answers. It uses the same Twitch,
Kick and Google apps as sign-in (the streamers' grants belong to them), so it gets the same client IDs
and secrets, and the same `DATA_ENCRYPTION_KEY` as the API.

- **Twitch** needs nothing more: the service connects out to Twitch (an EventSub conduit),
  no public URL involved.
- **Kick** delivers chat by webhook. In the Kick app's settings, turn webhooks on and set the
  URL to `https://noalbs.example.com/chat/webhooks/kick`. The dashboard container forwards it
  to the chat service.
- **YouTube** has no push events: while a switcher linked to a YouTube channel streams, one chat
  instance at a time looks for that channel's live broadcast (with the streamer's grant; right
  away, then every `CHAT_YOUTUBE__LIVE_CHECK_INTERVAL` seconds) and follows its chat over
  YouTube's gRPC stream (`youtube.googleapis.com`). So on YouTube, commands work while live.

**Several instances** (on one machine: `docker compose up -d --scale chat=2`; or on several
machines) share the work through NATS and Postgres, so they all need the same NATS and
database: Twitch's WebSocket shards are spread over them (`CHAT_TWITCH__SHARDS`, 4 by
default, is the most instances that get Twitch traffic), every chat message is handled by
exactly one of them, and when one stops the others take over its share. Kick's webhook can
go to any instance: send `/chat/webhooks/*` to them through your load balancer.
Environments (say staging and production) can share one NATS server: everything the chat
service puts in NATS is tagged with an ID stored in its database, so they never mix.

What the bot does besides commands, per switcher (Settings → Switching):
- **Chat notification** posts automatic scene switches ("Switched to LOW · BELABOX Cloud at
  640 kbps").
- **Stop stream after a raid** ends the stream when the streamer raids out; with **Announce raids**
  the bot also says so in chat. Kick has no webhook for raids, so one chat instance at a time
  watches those channels on Kick's own site WebSocket (`ws-us2.pusher.com`), looking their IDs
  up on `kick.com`; if Kick blocks that, the chat log says "Couldn't find a Kick channel's IDs".
- Streamers who remove NOALBS from their Twitch connections show up as "Needs reconnecting" on
  the Chat page; adding the channel again fixes it. Kick and YouTube grants that stop working
  show the same.

### "Add to NOALBS Cloud" links for stream server providers

A provider can link to `https://<domain>/add-server#kind=belabox&url=<encoded stats URL>&name=<optional>`
and the streamer picks a workspace and switcher and confirms. `https://<domain>/integrate-stream-server` explains the
format and has a link builder for providers. The data sits after the `#`, which browsers never send, so
stats URLs and keys stay out of the proxy and API logs. Nothing to configure; it needs no extra proxy route
(the SPA fallback serves both pages).

### Updating

```sh
docker compose pull && docker compose up -d
```

The migrator runs before the new API, obs-link and chat service start. Pin a version with
`NOALBS_TAG=sha-<commit>` in `.env`.

Schema changes ship as new migrations, applied in order by `db-migrator`; an existing database
is upgraded in place. Migrations only go forward, so to go back to an older version after an update
that changed the schema, restore the backup taken before it (see below).

### Backups

Back up Postgres (`docker compose exec postgres pg_dump -U noalbs noalbs > backup.sql`) and
keep `.env`: without `DATA_ENCRYPTION_KEY` the stored passwords in a backup are useless.
NATS only holds live data (the last two minutes of stats and current statuses), so it
doesn't need backing up.

### Observability

The API, obs-link and the chat service always log to stdout. With
`OTEL_EXPORTER_OTLP_ENDPOINT` set they also send logs, traces and metrics over OTLP/HTTP to
any OpenTelemetry backend (Grafana Cloud, Honeycomb, Axiom, an OpenTelemetry Collector or
Grafana Alloy of your own, …). The standard `OTEL_*` variables apply.

Grafana Cloud (the free tier is enough): in your stack, **Connections → OpenTelemetry
(OTLP)** gives the endpoint (`https://otlp-gateway-<region>.grafana.net/otlp`), the instance
ID and a token with write access for logs, traces and metrics. Then in `.env`:

```sh
OTEL_EXPORTER_OTLP_ENDPOINT=https://otlp-gateway-prod-eu-west-2.grafana.net/otlp
# base64 of "<instance ID>:<token>"; the space is written %20
OTEL_EXPORTER_OTLP_HEADERS=Authorization=Basic%20MTIzNDU2OmdsY18uLi4=
OTEL_RESOURCE_ATTRIBUTES=deployment.environment.name=production
```

What goes out is kept small:

- **Logs**: `info` and up (`LOG_EXPORT_FILTER`, `RUST_LOG` syntax, changes that), with
  `switcher.id`, `workspace.id`, `user.id` from the span they're in.
- **Traces**: a dashboard request (`GET /workspaces/{workspace_id}/…`, never the URL itself)
  or a chat command, with its database queries, through NATS to obs-link and OBS's answer.
  The routes every open dashboard polls (preview, OBS stats, sources, BELABOX details) aren't
  traced, and nothing per second is.
- **Metrics**: `http.server.request.duration`, `noalbs.obs.request.duration`,
  `noalbs.obslink.connections`, `noalbs.switchers` (connected, OBS connected, streaming,
  paused), `noalbs.switcher.scene_switches`, `noalbs.stream_server.fetches`,
  `noalbs.chat.commands`, `noalbs.chat.replies`. None carry a switcher or user ID.

A dashboard for all of it is in `grafana/noalbs.json`: in Grafana, **Dashboards → New → Import**
and upload the file; the Metrics and Logs pickers at its top choose your Prometheus and Loki data
sources (Grafana Cloud: `grafanacloud-<stack>-prom` and `-logs`). In Prometheus the
names use underscores and units (`noalbs_switchers`, `http_server_request_duration_seconds_bucket`).

Logs and traces hold account, workspace and switcher IDs and chat platform user IDs, never
IP addresses, tokens, stats URLs or chat messages. If you run NOALBS Cloud for others, your
backend is a processor of that data: name it in your privacy policy.

## Using your own reverse proxy instead of the dashboard's

If you'd rather route the services yourself, run the dashboard container anyway (it
serves the app) and set up these routes on one domain:

| Path | To | Notes |
|---|---|---|
| `/api/*` | `api:12000` | **Strip the `/api` prefix.** `GET …/live` is server-sent events: don't buffer it. |
| `/link` | `obs-link:11000` | WebSocket (upgrade headers). Connections stay open for as long as OBS runs. |
| `/moblin/*` | `obs-link:11000` | WebSocket, like `/link`: Moblin's remote control (optional). |
| `/chat/webhooks/*` | `chat:13000` | Kick's chat webhooks. Any chat instance. |
| everything else | `dashboard:80` | |

Set `CLIENT_IP_HEADER` to the header your proxy puts the visitor's IP in (nginx: `proxy_set_header
X-Real-IP $remote_addr;` and `x-real-ip`), or the API's rate limits see one address for everyone.

For nginx that looks like:

```nginx
location /api/ {
    proxy_pass http://api:12000/;          # the trailing slash strips /api
    proxy_http_version 1.1;
    proxy_buffering off;                   # live data (server-sent events)
    proxy_read_timeout 1h;
    proxy_set_header Host $host;
}
location = /link {
    proxy_pass http://obs-link:11000;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_read_timeout 1d;
}
location /moblin/ {
    proxy_pass http://obs-link:11000;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_read_timeout 1d;
}
location /chat/webhooks/ {
    proxy_pass http://chat:13000;
}
location / {
    proxy_pass http://dashboard:80;
}
```

## Configuration reference

Each service reads an optional `config.toml` from its working directory, then environment
variables, which win. Variables are `API_<KEY>` / `OBS_LINK_<KEY>`; nested keys use a double
underscore (`API_OAUTH_TWITCH__CLIENT_ID`). Empty variables count as unset. The defaults
are in each service's `src/config.rs`.

**API** (`API_…`)

| Key | Default | |
|---|---|---|
| `POSTGRES_URL` | `postgresql://postgres:postgres@localhost/localdb` | |
| `NATS_URL` | `nats://localhost:4222` | |
| `BASE_URL` | `http://localhost:12000` | The public address of the API, `https://<domain>/api`. OAuth callbacks are built from it. |
| `BROWSER_SOURCE_URL` | `http://localhost:5173/source/` | Where the dashboard serves the browser source, `https://<domain>/source/`. It connects to obs-link at `/link` on the same host. |
| `COOKIE_SECRET` | example key | 64 hex characters. Required in release builds. |
| `DATA_ENCRYPTION_KEY` | example key | 64 hex characters, same as obs-link's. Required in release builds. |
| `OAUTH_TWITCH__CLIENT_ID`, `__CLIENT_SECRET` | unset | |
| `OAUTH_KICK__CLIENT_ID`, `__CLIENT_SECRET` | unset | |
| `OAUTH_YOUTUBE__CLIENT_ID`, `__CLIENT_SECRET` | unset | The Google OAuth client (YouTube). |
| `OAUTH_YOUTUBE__API_KEY` | unset | A Google API key, to look YouTube channels up by handle (ignored chatters). |
| `OAUTH_TWITCH__AUTH_BASE`, `__API_BASE` (and `OAUTH_KICK__…`, `OAUTH_YOUTUBE__…`) | the platform | Only for tests against a fake OAuth server. |
| `DASHBOARD_URL` | `http://localhost:5173` | Where the dashboard is served, `https://<domain>`. Stripe sends people back here. |
| `STRIPE__SECRET_KEY`, `__WEBHOOK_SECRET` | unset | Billing is off without them (see [Billing](#billing-stripe)). |
| `STRIPE__API_BASE` | `https://api.stripe.com` | Only for tests against a fake Stripe. |
| `DEV_LOGIN` | `false` | Sign in with any name, no OAuth. Local testing only. |
| `ALLOW_PRIVATE_STATS_URLS` | `false` | Let "Test connection" reach private addresses and save them. Local testing or servers on your own network only (set the same on obs-link). |
| `CLIENT_IP_HEADER` | unset (compose: `x-forwarded-for`) | The header the proxy in front puts the visitor's IP in, for rate limits: `x-forwarded-for` behind the dashboard's Caddy, `cf-connecting-ip` behind Cloudflare. Unset: the connection's address. |
| `HOST`, `PORT` | `0.0.0.0`, `12000` | |
| `POSTGRES_MAX_CONNECTIONS` | `10` | |
| `NATS_REQUEST_TIMEOUT` | `5000` | Milliseconds to wait for OBS to answer. |

**obs-link** (`OBS_LINK_…`)

| Key | Default | |
|---|---|---|
| `POSTGRES_URL` | `postgres://postgres:postgres@localhost/localdb` | |
| `NATS_URL` | `nats://localhost:4222` | |
| `DATA_ENCRYPTION_KEY` | example key | Same as the API's. Required in release builds. |
| `HOST`, `PORT` | `0.0.0.0`, `11000` | |
| `POSTGRES_MAX_CONNECTIONS` | `10` | |
| `BELABOX_URL` | `wss://remote.belabox.net/ws/remote` | BELABOX Cloud's remote endpoint (outbound WebSocket, for switchers with a BELABOX remote key). |
| `ALLOW_PRIVATE_STATS_URLS` | `false` | Poll stats URLs on private addresses. Local testing or a self-hosted setup with servers on your own network only. |

**chat** (`CHAT_…`)

| Key | Default | |
|---|---|---|
| `POSTGRES_URL` | `postgres://postgres:postgres@localhost/localdb` | |
| `NATS_URL` | `nats://localhost:4222` | |
| `DATA_ENCRYPTION_KEY` | example key | Same as the API's. Required in release builds. |
| `TWITCH__CLIENT_ID`, `__CLIENT_SECRET` | unset | The API's Twitch app. Twitch chat is off without it. |
| `TWITCH__SHARDS` | `4` | EventSub conduit shards (WebSockets), spread over the instances. |
| `KICK__CLIENT_ID`, `__CLIENT_SECRET` | unset | The API's Kick app. Kick chat is off without it. |
| `YOUTUBE__CLIENT_ID`, `__CLIENT_SECRET` | unset | The API's Google OAuth client. YouTube chat is off without it. |
| `YOUTUBE__API_KEY` | unset | For `!noalbs ignore <handle>` on YouTube. |
| `YOUTUBE__LIVE_CHECK_INTERVAL` | `60` | Seconds between looks for a streaming channel's live broadcast (1 quota unit each), after three quicker ones. |
| `INSTANCE_ID` | random | Names the instance in logs and shard claims; unique per running instance. |
| `HOST`, `PORT` | `0.0.0.0`, `13000` | The Kick webhook and `GET /health`. |
| `POSTGRES_MAX_CONNECTIONS` | `10` | |
| `NATS_REQUEST_TIMEOUT` | `5000` | Milliseconds to wait for OBS to answer. |
| `TWITCH__API_BASE`, `__AUTH_BASE`, `__EVENTSUB_URL`, `KICK__API_BASE`, `__AUTH_BASE`, `__PUBLIC_KEY`, `__WEB_BASE`, `__PUSHER_URL`, `YOUTUBE__API_BASE`, `__AUTH_BASE`, `__GRPC_URL` | the platforms | Only for tests against a fake (`cargo run -p chat --example fake_platforms`). |

**db-migrator**: `DATABASE_URL`.

Logging (api, obs-link, chat): `RUST_LOG`, e.g. `info` (the images' default) or
`api=debug,info`; `LOG_FORMAT=json` for JSON lines. Export: `OTEL_EXPORTER_OTLP_ENDPOINT`,
`OTEL_EXPORTER_OTLP_HEADERS`, `OTEL_RESOURCE_ATTRIBUTES`, `LOG_EXPORT_FILTER` (see
"Observability"), not prefixed.

## What the machine needs to reach

- **obs-link** fetches each stream server's stats URL (BELABOX Cloud, your MediaMTX, …), so
  it needs outbound internet access. Stats URLs are typed by users, so obs-link refuses
  private, loopback and link-local addresses (checked each time a name is looked up) and
  follows no redirects, unless `ALLOW_PRIVATE_STATS_URLS=true` (which compose gives to the API
  and obs-link alike). Self-hosting with stream servers on your own network needs that flag.
  Also block egress to your cluster and cloud metadata ranges from obs-link's pods.
- **The API** does the same for "Test connection" and refuses to save a stats URL that is a
  private IP address.
- **The chat service** talks to Twitch (`api.twitch.tv`, `id.twitch.tv`,
  `eventsub.wss.twitch.tv`) and Kick (`api.kick.com`, `id.kick.com`, and for raids `kick.com`
  and `ws-us2.pusher.com`) and YouTube (`oauth2.googleapis.com`, `www.googleapis.com`,
  `youtube.googleapis.com` over gRPC), and Kick has to reach its webhook URL.
- **OBS** (the streamer's PC) loads the browser source from `NOALBS_PUBLIC_URL` and connects
  to `/link` on the same domain. Nothing needs to reach the streamer's PC.
- **Moblin** (optional) connects to `wss://…/moblin/<switcher ID>` on the same domain; its
  Assistant URL is derived from `BROWSER_SOURCE_URL`.
