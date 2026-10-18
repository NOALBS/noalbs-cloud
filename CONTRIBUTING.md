# Contributing

Thanks for wanting to help. Bug reports, fixes and new stream server kinds are all welcome.

- **Bugs:** open an issue with what you did, what you expected and what happened. Leave out stats URLs,
  stream keys, browser-source links and IP addresses.
- **Bigger changes** (a new feature, a new dependency, a change to how switching decides): open an issue or
  ask on [Discord](https://discord.gg/efWu5HWM2u) first, so you don't build something that can't be merged.
- **Security problems:** see [SECURITY.md](SECURITY.md), not the issue tracker.

## Working on the code

The [README](README.md) gets it running; [Local development](#local-development) below has the rest.
[Architecture](https://docs.noalbs.com/contributing/architecture/) in the docs explains how the
services fit together, and each module's doc comment explains its part.

Before opening a pull request, run what CI runs:

    cargo fmt --all -- --check
    cargo build --workspace --locked
    DATABASE_URL=postgres://postgres:postgres@localhost:5432/localdb cargo test --workspace --locked
    cd web/dashboard && pnpm typecheck && pnpm lint && pnpm test && pnpm build

A few rules that are easy to miss:

- **Never edit a migration that has shipped.** Add a new file in `crates/db/migrations/`, and keep it
  backward-compatible (the old services run against the new schema for a moment during a deploy).
- After changing API routes or DTOs, regenerate the dashboard's types:
  `cargo run -p api -- --openapi > web/dashboard/openapi.json && (cd web/dashboard && pnpm gen:api)`.
- Switching rules live in `services/obs-link/src/decide.rs` and match NOALBS's; a change there
  comes with a test.
- Stats URLs, stream keys and IPs are secrets: keep them out of `info` logs, span fields and plain view in
  the dashboard (streamers show it on stream).
- Ported code from another project keeps its notice; see [`docs/licenses.md`](docs/licenses.md).

## AI-assisted contributions

NOALBS Cloud is developed with help from AI coding tools, and you're welcome to use them too. You're
responsible for what you submit: read and understand every line, run the checks, and make sure you
have the right to contribute it under the AGPL. Say in the pull request if most of it was generated.
Pull requests that look generated and unreviewed will be closed.

## Local development

### Settings

Each service reads an optional `config.toml` from the directory you start it in (gitignored), then
environment variables, which win: `API_<KEY>`, `OBS_LINK_<KEY>` or `CHAT_<KEY>`, with `__` for nested
keys (`API_OAUTH_TWITCH__CLIENT_ID`). Empty variables count as unset. The defaults, and every key, are
in each service's `src/config.rs`; [`deploy/README.md`](deploy/README.md) lists them too. Debug builds
accept the example `cookie_secret` and `data_encryption_key`; release builds refuse them. The API and
obs-link must use the same `data_encryption_key`.

- `dev_login = true` (API): sign in with any name, no OAuth apps needed. From the command line:
  `curl -c jar -H 'content-type: application/json' -d '{"username":"me"}' localhost:12000/auth/dev-login`.
- `allow_private_stats_urls = true` (API **and** obs-link): read a stream server on your machine or
  network. Without it, "Test connection" and the switcher refuse private addresses.
- The dashboard's Vite server proxies `/api` to the API and `/link` to obs-link (`NOALBS_API_URL` and
  `NOALBS_OBS_LINK_URL` override where). Set the API's `base_url = "http://localhost:5173/api"` so
  OAuth callbacks and cookies stay on the dashboard's address. New switchers show their browser-source
  link, served by the dashboard at `/source/`.
- Some test data: `cargo run -p db --example create_user_workspace_switcher_streamserver`.
- Logs: `RUST_LOG` (e.g. `api=debug,info`), `LOG_FORMAT=json`.

### Fakes

**Twitch, Kick and YouTube**: `cargo run -p chat --example fake_platforms` (port 18100) fakes their
APIs, EventSub, Kick's signed webhooks, Google's sign-in and YouTube's chat stream. Point the chat
service at it:

    CHAT_TWITCH__API_BASE=http://localhost:18100 CHAT_TWITCH__AUTH_BASE=http://localhost:18100 \
    CHAT_TWITCH__EVENTSUB_URL=ws://localhost:18100/ws \
    CHAT_KICK__API_BASE=http://localhost:18100 CHAT_KICK__AUTH_BASE=http://localhost:18100 \
    CHAT_KICK__WEB_BASE=http://localhost:18100 CHAT_KICK__PUSHER_URL=ws://localhost:18100/app/fake \
    cargo run -p chat

and drive it with `POST /control/twitch/chat`, `/control/kick/chat` (also `raid`, `revoke`,
`reconnect`; the example's doc comment has them all). To link channels and bots through it, point the
API's `oauth_twitch` / `oauth_kick` `auth_base` and `api_base` at it too; `/control/next` picks the
account the next sign-in gets. Give it a NATS of its own if other environments share yours.

**BELABOX Cloud**: `cargo run -p obs-link --example fake_belabox` (port 18200, `PORT` overrides) is one
BELABOX with modems, WiFi, a hotspot and sensors. Run obs-link with
`OBS_LINK_BELABOX_URL=ws://localhost:18200` and save the remote key `test` on a switcher. Type
`offline`, `online` or `drop` into its terminal.

**A Moblin phone**: set up Moblin on a switcher, then
`PASSWORD=<its password> cargo run -p obs-link --example fake_moblin -- <switcher ID>` connects as the
phone. Type `live`, `stop`, `mute`, `unmute`, `scene`, `torch` or `drop`. A real phone on your network
needs the dashboard reachable (`pnpm dev --host`) and the API's `browser_source_url` on your network
address.

### Billing

Put Stripe test-mode keys in the API's `config.toml` (`[stripe] secret_key`, `webhook_secret`), make
yourself staff (`UPDATE users SET staff = true …`), set prices under Admin → Products, and forward
webhooks with `stripe listen --forward-to localhost:12000/billing/webhook` (its `whsec_…` is the
`webhook_secret`). The test card is `4242 4242 4242 4242`.

### Traces and metrics

`docker run --rm -p 3000:3000 -p 4318:4318 grafana/otel-lgtm` gives Grafana with logs, traces and
metrics at http://localhost:3000; start the services with
`OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318`.

## License of contributions

NOALBS Cloud is licensed under the [GNU AGPL v3.0 or later](LICENSE). By sending a pull request you agree
that your contribution is licensed under the same license.
