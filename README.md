# NOALBS Cloud

The hosted version of [NOALBS](https://github.com/NOALBS/nginx-obs-automatic-low-bitrate-switching),
the automatic OBS scene switcher for IRL streams. NOALBS watches the stream server your encoder
(BELABOX, IRL Pro, Moblin, ...) sends to and switches OBS between LIVE, LOW and BRB scenes when the
bitrate or latency drops. NOALBS Cloud does the same without running anything on your PC: one browser
source in OBS connects it to the service.

On top of the switching it has a dashboard you can use from your phone (scenes, sources, audio mixer,
stream start/stop), stream history with markers, Twitch, Kick and YouTube chat commands, BELABOX Cloud
and Moblin remote control, and workspaces to share a setup with moderators.

[Docs](https://docs.noalbs.com) · [Discord](https://discord.gg/efWu5HWM2u) · [X](https://x.com/noalbs)

## Running it locally

With the Nix flake (`direnv allow`) you get every tool; otherwise install stable Rust, Node 24, pnpm and
Docker.

```sh
docker compose -f scripts/docker-compose.yml up -d     # Postgres and NATS
DATABASE_URL=postgres://postgres:postgres@localhost:5432/localdb cargo run -p db-migrator
echo 'dev_login = true' > config.toml                   # sign in without OAuth apps
cargo run -p api          # :12000
cargo run -p obs-link     # :11000
cd web/dashboard && pnpm install && pnpm dev           # http://localhost:5173
```

The sign-in page then has a dev login that takes any username. There are fakes for the parts you'd
otherwise need accounts or devices for: Twitch, Kick and YouTube, BELABOX Cloud and a Moblin phone.
[CONTRIBUTING.md](CONTRIBUTING.md#local-development) has the details, and the docs explain
[how the parts fit together](https://docs.noalbs.com/contributing/architecture/).

## Hosting your own

[`deploy/README.md`](deploy/README.md) runs the whole stack with Docker Compose from the published images.
If you run it for other people:

- Replace the privacy policy and terms (`web/dashboard/privacy.html`, `terms.html`): they describe our
  service and name us as its operator. Change `support@noalbs.com` in the dashboard too.
- The AGPL asks that your users can get the source of what you run: point the link on `/licenses`
  (`web/dashboard/licenses.html`) at your fork.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Found a security problem? Please follow [SECURITY.md](SECURITY.md)
rather than opening an issue.
