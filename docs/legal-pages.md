# Legal pages

Three static pages in `web/dashboard/`, built as extra Vite entries (styles through `src/legal/main.ts`,
long-form styles under `.legal` in `src/styles.css`) and served by Caddy:

| URL         | File                                             |
| ----------- | ------------------------------------------------ |
| `/privacy`  | `privacy.html`                                   |
| `/terms`    | `terms.html`                                     |
| `/licenses` | `licenses.html` (see [licenses.md](licenses.md)) |

A new static page needs: the file, an `input` entry and a short URL in `vite.config.ts`, a `handle` in
`Caddyfile`, an entry in `RESERVED_SLUGS` (`services/api/src/validate.rs`) and in `NOT_THE_APP`
(`src/lib/redirect.ts`), with their tests, and links in the footers.

## Update the privacy policy when

- we store new personal data, or stop storing some;
- a retention period changes (history days per plan, sessions, logs, payment records);
- we add or change a service that handles personal data (hosting, Cloudflare, Stripe, email (Migadu), logs and
  traces (Grafana Labs and Axiom, whatever Alloy in the cluster sends to), chat platforms,
  BELABOX, Moblin, ...);
- support or staff powers change (support access, suspension, break-glass);
- organizations change what their team can see or do in a client workspace, or who may leave one.

## Update the terms when

- plans, prices, billing rules, refunds or what happens on a failed payment change;
- staff may do something new to accounts, workspaces or organizations (pause, suspend, disable);
- the rules of managed workspaces change (who pays, what a customer can do, what the team sees);
- we add a feature that acts for the streamer (starts or stops streams, posts in chat, controls devices);
- the operator changes (a company replaces Brian Spit: name, registration number, address, court).

Change the "Last updated" date at the top of the page you edit. For a change that matters to users, say
so in the dashboard or on Discord, as the pages promise.